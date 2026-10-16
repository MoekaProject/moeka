//! Full-text candidate index for workspace search, backed by tantivy.
//!
//! The index is a pure superset filter: files are indexed as lowercase
//! character 2-grams, a query is reduced to the same 2-grams and matched with
//! an AND query, and the resulting candidate paths are then scanned with the
//! exact matcher (`find_search_matches`). This keeps the search semantics
//! (substring / case sensitivity / whole word / regex) identical to a full
//! scan while skipping files that cannot possibly match.

use anyhow::Context as _;
use std::io::Read;
use std::path::{Path, PathBuf};

use tantivy::collector::DocSetCollector;
use tantivy::query::{BooleanQuery, Occur, Query, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::tokenizer::{LowerCaser, NgramTokenizer, TextAnalyzer};
use tantivy::{Index, IndexReader, TantivyDocument, Term};

use super::{classify_file_kind_for_bytes, should_skip_path};

const NGRAM_TOKENIZER: &str = "moeka_ngram2";
const INDEX_WRITER_BUDGET_BYTES: usize = 50_000_000;
/// Files larger than this are not indexed and are always scanned instead.
const MAX_INDEXED_FILE_BYTES: usize = 4 * 1024 * 1024;
/// Bounds source text retained while constructing the in-memory n-gram index.
const MAX_INDEXED_WORKSPACE_BYTES: usize = 64 * 1024 * 1024;
const FILE_KIND_SAMPLE_BYTES: usize = 4096;

pub(crate) struct WorkspaceSearchIndex {
    root: PathBuf,
    index: Index,
    reader: IndexReader,
    path_field: Field,
    content_field: Field,
    /// Text-searchable files that were skipped during indexing (too large);
    /// they are appended to every candidate set so results stay complete.
    unindexed_paths: Vec<PathBuf>,
}

fn ngram_analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(NgramTokenizer::new(2, 2, false).expect("valid ngram bounds"))
        .filter(LowerCaser)
        .build()
}

impl WorkspaceSearchIndex {
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Walk the workspace and build an in-memory index of all text-searchable
    /// files. Runs on a background thread.
    pub(crate) fn build(root: &Path) -> anyhow::Result<Self> {
        crate::platform::fs_task::check_deadline()?;
        if !std::fs::metadata(root)
            .with_context(|| format!("Inspect workspace {}", root.display()))?
            .is_dir()
        {
            anyhow::bail!("Workspace {} is not a directory", root.display());
        }
        let mut schema_builder = Schema::builder();
        let path_field = schema_builder.add_text_field("path", STRING | STORED);
        let content_options = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(NGRAM_TOKENIZER)
                .set_index_option(IndexRecordOption::Basic),
        );
        let content_field = schema_builder.add_text_field("content", content_options);
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema);
        index
            .tokenizers()
            .register(NGRAM_TOKENIZER, ngram_analyzer());

        let mut writer = index.writer::<TantivyDocument>(INDEX_WRITER_BUDGET_BYTES)?;
        let mut unindexed_paths = Vec::new();
        let mut indexed_source_bytes = 0usize;
        let mut pending = vec![root.to_path_buf()];
        while let Some(path) = pending.pop() {
            crate::platform::fs_task::check_deadline()?;
            let observed = if path == root {
                std::fs::metadata(&path)
            } else {
                std::fs::symlink_metadata(&path)
            };
            let metadata = match observed {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| format!("Inspect {}", path.display()));
                }
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            let is_dir = metadata.is_dir();
            if path != root && should_skip_path(&path, is_dir) {
                continue;
            }
            if is_dir {
                for entry in
                    std::fs::read_dir(&path).with_context(|| format!("List {}", path.display()))?
                {
                    crate::platform::fs_task::check_deadline()?;
                    pending.push(entry?.path());
                }
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            let should_skip_full_read = metadata.len() > MAX_INDEXED_FILE_BYTES as u64
                || metadata.len()
                    > MAX_INDEXED_WORKSPACE_BYTES.saturating_sub(indexed_source_bytes) as u64;
            if should_skip_full_read {
                let mut sample = [0; FILE_KIND_SAMPLE_BYTES];
                let mut file = std::fs::File::open(&path)
                    .with_context(|| format!("Open {}", path.display()))?;
                crate::platform::fs_task::check_deadline()?;
                let sample_len = file
                    .read(&mut sample)
                    .with_context(|| format!("Read {}", path.display()))?;
                if classify_file_kind_for_bytes(&path, &sample[..sample_len]).supports_text_search()
                {
                    unindexed_paths.push(path);
                }
                continue;
            }
            let mut bytes = Vec::new();
            std::fs::File::open(&path)
                .with_context(|| format!("Open {}", path.display()))?
                .take(MAX_INDEXED_FILE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .with_context(|| format!("Read {}", path.display()))?;
            crate::platform::fs_task::check_deadline()?;
            if !classify_file_kind_for_bytes(&path, &bytes).supports_text_search() {
                continue;
            }
            if bytes.len() > MAX_INDEXED_FILE_BYTES {
                unindexed_paths.push(path);
                continue;
            }
            indexed_source_bytes = indexed_source_bytes.saturating_add(bytes.len());
            let content = String::from_utf8_lossy(&bytes);
            let mut document = TantivyDocument::default();
            document.add_text(path_field, path.to_string_lossy());
            document.add_text(content_field, content);
            writer.add_document(document)?;
        }
        crate::platform::fs_task::check_deadline()?;
        writer.commit()?;
        crate::platform::fs_task::check_deadline()?;

        let reader = index.reader()?;
        Ok(Self {
            root: root.to_path_buf(),
            index,
            reader,
            path_field,
            content_field,
            unindexed_paths,
        })
    }

    /// Lowercase 2-grams of the query, produced by the same analyzer used for
    /// indexing. Returns `None` when the query is too short to gram.
    fn query_grams(&self, query: &str) -> Option<Vec<String>> {
        let mut analyzer = self.index.tokenizers().get(NGRAM_TOKENIZER)?;
        let mut stream = analyzer.token_stream(query);
        let mut grams = Vec::new();
        while let Some(token) = stream.next() {
            if !grams.contains(&token.text) {
                grams.push(token.text.clone());
            }
        }
        if grams.is_empty() { None } else { Some(grams) }
    }

    /// Candidate file paths that may contain `query` as a substring
    /// (case-insensitive superset). Returns `None` when the index cannot
    /// answer (regex queries, or queries shorter than one 2-gram) and the
    /// caller must fall back to a full scan.
    pub(crate) fn candidate_paths(&self, query: &str, use_regex: bool) -> Option<Vec<PathBuf>> {
        if use_regex {
            return None;
        }
        let grams = self.query_grams(query)?;
        let subqueries: Vec<(Occur, Box<dyn Query>)> = grams
            .into_iter()
            .map(|gram| {
                let term = Term::from_field_text(self.content_field, &gram);
                let query: Box<dyn Query> =
                    Box::new(TermQuery::new(term, IndexRecordOption::Basic));
                (Occur::Must, query)
            })
            .collect();
        let boolean_query = BooleanQuery::new(subqueries);

        let searcher = self.reader.searcher();
        let doc_addresses = searcher.search(&boolean_query, &DocSetCollector).ok()?;
        let mut paths = Vec::with_capacity(doc_addresses.len() + self.unindexed_paths.len());
        for address in doc_addresses {
            let Ok(document) = searcher.doc::<TantivyDocument>(address) else {
                continue;
            };
            if let Some(path) = document
                .get_first(self.path_field)
                .and_then(|value| value.as_str())
            {
                paths.push(PathBuf::from(path));
            }
        }
        paths.extend(self.unindexed_paths.iter().cloned());
        paths.sort();
        paths.dedup();
        Some(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::SystemTime;

    fn temp_workspace(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "moeka-search-index-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn index_filters_candidates_for_ascii_and_cjk_queries() {
        let root = temp_workspace("basic");
        fs::write(root.join("a.md"), "hello world").unwrap();
        fs::write(root.join("b.md"), "文本编辑器实现").unwrap();
        fs::write(root.join("c.md"), "nothing relevant").unwrap();

        let index = WorkspaceSearchIndex::build(&root).unwrap();

        let ascii = index.candidate_paths("Hello", false).unwrap();
        assert_eq!(ascii, vec![root.join("a.md")]);

        let cjk = index.candidate_paths("编辑器", false).unwrap();
        assert_eq!(cjk, vec![root.join("b.md")]);

        // Regex and single-character queries fall back to a full scan.
        assert!(index.candidate_paths("hello", true).is_none());
        assert!(index.candidate_paths("h", false).is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn index_skips_hidden_and_binary_files() {
        let root = temp_workspace("skip");
        fs::write(root.join(".hidden.md"), "needle").unwrap();
        fs::write(root.join("bin.dat"), [0u8, 159, 146, 150, 0, 1]).unwrap();
        fs::write(root.join("real.md"), "needle").unwrap();

        let index = WorkspaceSearchIndex::build(&root).unwrap();
        let candidates = index.candidate_paths("needle", false).unwrap();
        assert_eq!(candidates, vec![root.join("real.md")]);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_root_does_not_build_a_successful_empty_index() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("not-a-directory.md");
        fs::write(&path, "text").unwrap();
        assert!(WorkspaceSearchIndex::build(&path).is_err());
        assert!(WorkspaceSearchIndex::build(&directory.path().join("missing")).is_err());
    }

    #[test]
    fn selected_root_is_not_subject_to_descendant_exclusions() {
        let directory = tempfile::tempdir().unwrap();
        for name in [".workspace", "target"] {
            let root = directory.path().join(name);
            fs::create_dir(&root).unwrap();
            fs::write(root.join("note.md"), "needle").unwrap();
            fs::write(root.join(".hidden.md"), "needle").unwrap();
            for excluded in [".hidden", "target", "node_modules"] {
                fs::create_dir(root.join(excluded)).unwrap();
                fs::write(root.join(excluded).join("ignored.md"), "needle").unwrap();
            }
            let index = WorkspaceSearchIndex::build(&root).unwrap();
            assert_eq!(
                index.candidate_paths("needle", false).unwrap(),
                vec![root.join("note.md")]
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn explicitly_selected_symlink_root_is_indexed() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        let alias = directory.path().join(".alias");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("note.md"), "needle").unwrap();
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        let index = WorkspaceSearchIndex::build(&alias).unwrap();
        assert_eq!(
            index.candidate_paths("needle", false).unwrap(),
            vec![alias.join("note.md")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn index_does_not_follow_directory_cycles() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("note.md"), "needle").unwrap();
        std::os::unix::fs::symlink(root.path(), root.path().join("cycle")).unwrap();
        let index = WorkspaceSearchIndex::build(root.path()).unwrap();
        assert_eq!(
            index.candidate_paths("needle", false).unwrap(),
            vec![root.path().join("note.md")]
        );
    }

    #[test]
    fn oversized_text_files_remain_search_candidates() {
        let root = temp_workspace("oversized");
        let large_text = root.join("large.md");
        let file = fs::File::create(&large_text).unwrap();
        file.set_len(MAX_INDEXED_FILE_BYTES as u64 + 1).unwrap();
        let large_binary = root.join("large.bin");
        let binary = fs::File::create(&large_binary).unwrap();
        binary.set_len(MAX_INDEXED_FILE_BYTES as u64 + 1).unwrap();

        let index = WorkspaceSearchIndex::build(&root).unwrap();
        let candidates = index.candidate_paths("needle", false).unwrap();
        assert_eq!(candidates, vec![large_text]);

        fs::remove_dir_all(root).unwrap();
    }
}
