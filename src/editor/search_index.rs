//! Full-text candidate index for workspace search, backed by tantivy.
//!
//! The index is a pure superset filter: files are indexed as lowercase
//! character 2-grams, a query is reduced to the same 2-grams and matched with
//! an AND query, and the resulting candidate paths are then scanned with the
//! exact matcher (`find_search_matches`). This keeps the search semantics
//! (substring / case sensitivity / whole word / regex) identical to a full
//! scan while skipping files that cannot possibly match.

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
        let mut pending = vec![root.to_path_buf()];
        while let Some(path) = pending.pop() {
            let is_dir = path.is_dir();
            if should_skip_path(&path, is_dir) {
                continue;
            }
            if is_dir {
                if let Ok(entries) = std::fs::read_dir(&path) {
                    pending.extend(entries.flatten().map(|entry| entry.path()));
                }
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if !classify_file_kind_for_bytes(&path, &bytes).supports_text_search() {
                continue;
            }
            if bytes.len() > MAX_INDEXED_FILE_BYTES {
                unindexed_paths.push(path);
                continue;
            }
            let content = String::from_utf8_lossy(&bytes);
            let mut document = TantivyDocument::default();
            document.add_text(path_field, path.to_string_lossy());
            document.add_text(content_field, content);
            writer.add_document(document)?;
        }
        writer.commit()?;

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
}
