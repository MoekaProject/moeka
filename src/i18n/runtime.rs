//! First-party runtime messages keep their identity until formatted. External
//! OS/library errors and document-controlled values are appended unchanged.
use super::{UiLanguage, command_label_index};
use std::fmt;
#[cfg(not(test))]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(not(test))]
static LANGUAGE: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
thread_local! {
    // Independent GPUI apps in parallel tests must not change each other's locale.
    static LANGUAGE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn set_language(language: UiLanguage) {
    #[cfg(not(test))]
    LANGUAGE.store(command_label_index(language), Ordering::Relaxed);
    #[cfg(test)]
    LANGUAGE.set(command_label_index(language));
}

pub(crate) fn language() -> UiLanguage {
    #[cfg(not(test))]
    let index = LANGUAGE.load(Ordering::Relaxed);
    #[cfg(test)]
    let index = LANGUAGE.get();
    UiLanguage::supported()[index]
}

/// Operation identifiers remain stable for admission/rate limiting; only their
/// presentation is localized. Unknown internal operations use the generic label.
pub(crate) fn operation_text(operation: &str) -> RuntimeText {
    match operation {
        "save"
        | "save as"
        | "save and close"
        | "auto-save"
        | "Save before adding to chat"
        | "save undo history" => RuntimeText::WriteFile,
        "Save settings" => RuntimeText::SaveSettings,
        "copy"
        | "copy selected image"
        | "import clipboard resources"
        | "import dropped attachments" => RuntimeText::CopyFile,
        "rename" | "move" => RuntimeText::MoveFile,
        "delete" => RuntimeText::DeleteFile,
        "create folder" => RuntimeText::CreateDirectory,
        "create file" => RuntimeText::WriteFile,
        "render HTML export" | "prepare clipboard export" => RuntimeText::PrepareExport,
        "Render native export" => RuntimeText::RenderFailed,
        "write export" => RuntimeText::SaveExport,
        "publish clipboard export" | "copy image to clipboard" => RuntimeText::ClipboardFailed,
        "load document image" | "Read file list thumbnail" => RuntimeText::ImageRead,
        "read document"
        | "read export source"
        | "read file information"
        | "Read file list preview"
        | "Read file list previews"
        | "Refresh file list previews" => RuntimeText::ReadFile,
        "Open launch path" | "Open preview tab" | "Open tab" | "Restore tab" => {
            RuntimeText::OpenFile
        }
        "Read sidebar directory"
        | "Read file list directory"
        | "Expand sidebar directories"
        | "Count directory documents" => RuntimeText::ReadDirectory,
        "Search workspace"
        | "Index workspace"
        | "Prepare search"
        | "Find files"
        | "Continue search"
        | "search wiki links"
        | "search recent wiki links"
        | "Filter sidebar files" => RuntimeText::SearchFiles,
        "resolve Markdown link"
        | "resolve wiki link target"
        | "Check file"
        | "validate terminal link"
        | "reveal referenced file" => RuntimeText::InspectPath,
        _ => RuntimeText::FileAction,
    }
}

macro_rules! catalog {
    ($($key:ident => $values:expr),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[allow(dead_code)] // Some messages are used only by other platform backends.
        pub(crate) enum RuntimeText { $($key),+ }
        impl RuntimeText {
            #[cfg(test)]
            const ALL: &'static [Self] = &[$(Self::$key),+];
            pub(crate) fn text(self, language: UiLanguage) -> &'static str {
                let values: [&str; 12] = match self { $(Self::$key => $values),+ };
                values[command_label_index(language)]
            }
        }
    };
}

catalog! {
    TransferConflict => ["The transfer destination already exists; it was not overwritten", "传输目标已存在，未覆盖原内容", "傳輸目標已存在，未覆寫原內容", "転送先は既に存在するため上書きしませんでした", "전송 대상이 이미 있어 덮어쓰지 않았습니다", "La destination existe déjà ; elle n'a pas été écrasée", "Das Übertragungsziel existiert bereits und wurde nicht überschrieben", "El destino ya existe; no se sobrescribió", "O destino já existe; não foi sobrescrito", "Назначение уже существует; перезапись не выполнена", "وجهة النقل موجودة بالفعل؛ لم تتم الكتابة فوقها", "स्थानांतरण का गंतव्य पहले से मौजूद है; उसे अधिलेखित नहीं किया गया"],
    TransferChanged => ["The source or destination changed during transfer", "传输期间源或目标发生了变化", "傳輸期間來源或目標發生變更", "転送中に転送元または転送先が変更されました", "전송 중 원본 또는 대상이 변경되었습니다", "La source ou la destination a changé pendant le transfert", "Quelle oder Ziel wurde während der Übertragung geändert", "El origen o destino cambió durante la transferencia", "A origem ou o destino mudou durante a transferência", "Источник или назначение изменились при передаче", "تغير المصدر أو الوجهة أثناء النقل", "स्थानांतरण के दौरान स्रोत या गंतव्य बदल गया"],
    TransferUncertain => ["The transfer outcome is uncertain; verify both locations before retrying", "传输结果不确定，请核实源和目标后再重试", "傳輸結果不確定，請確認來源與目標後再重試", "転送結果が不明です。再試行する前に両方の場所を確認してください", "전송 결과가 불확실합니다. 다시 시도하기 전에 양쪽 위치를 확인하세요", "Résultat du transfert incertain ; vérifiez les deux emplacements avant de réessayer", "Übertragungsergebnis unklar; vor einem erneuten Versuch beide Orte prüfen", "El resultado es incierto; verifique ambas ubicaciones antes de reintentar", "Resultado incerto; verifique ambos os locais antes de tentar novamente", "Результат передачи неизвестен; проверьте оба расположения перед повтором", "نتيجة النقل غير مؤكدة؛ تحقق من الموقعين قبل إعادة المحاولة", "स्थानांतरण का परिणाम अनिश्चित है; फिर प्रयास करने से पहले दोनों स्थान जाँचें"],
    TransferCleanupFailed => ["Could not clean up the transfer session", "无法清理传输会话", "無法清理傳輸工作階段", "転送セッションをクリーンアップできませんでした", "전송 세션을 정리하지 못했습니다", "Impossible de nettoyer la session de transfert", "Übertragungssitzung konnte nicht bereinigt werden", "No se pudo limpiar la sesión de transferencia", "Não foi possível limpar a sessão de transferência", "Не удалось очистить сеанс передачи", "تعذر تنظيف جلسة النقل", "स्थानांतरण सत्र साफ़ नहीं किया जा सका"],
    TransferCopyRetained => ["Any created destination is retained. The write outcome may be uncertain; verify before retrying", "已创建的目标将保留。写入结果可能不确定，请核实后再重试", "已建立的目標將保留。寫入結果可能不確定，請確認後再重試", "作成済みの転送先は保持されます。書き込み結果が不明な場合があるため再試行前に確認してください", "생성된 대상은 유지됩니다. 쓰기 결과가 불확실할 수 있으니 다시 시도하기 전에 확인하세요", "Toute destination créée est conservée. L'écriture peut être incertaine ; vérifiez avant de réessayer", "Erstellte Ziele bleiben erhalten. Schreibergebnis möglicherweise unklar; vor Wiederholung prüfen", "Se conserva el destino creado. La escritura puede ser incierta; verifique antes de reintentar", "O destino criado é preservado. A gravação pode ser incerta; verifique antes de tentar novamente", "Созданное назначение сохранено. Результат записи может быть неизвестен; проверьте перед повтором", "يتم الاحتفاظ بأي وجهة تم إنشاؤها. قد تكون نتيجة الكتابة غير مؤكدة؛ تحقق قبل إعادة المحاولة", "बनाया गया गंतव्य सुरक्षित है। लिखने का परिणाम अनिश्चित हो सकता है; फिर प्रयास करने से पहले जाँचें"],
    TransferMoveRetained => ["Any created destination is retained. Source removal or rename may be uncertain; verify before retrying", "已创建的目标将保留。源的删除或重命名结果可能不确定，请核实后再重试", "已建立的目標將保留。來源刪除或重新命名的結果可能不確定，請確認後再重試", "作成済みの転送先は保持されます。転送元の削除や名前変更の結果が不明な場合があるため再試行前に確認してください", "생성된 대상은 유지됩니다. 원본 삭제 또는 이름 변경 결과가 불확실할 수 있으니 다시 시도하기 전에 확인하세요", "Toute destination créée est conservée. La suppression ou le renommage de la source peut être incertain ; vérifiez avant de réessayer", "Erstellte Ziele bleiben erhalten. Löschung oder Umbenennung der Quelle möglicherweise unklar; vor Wiederholung prüfen", "Se conserva el destino creado. La eliminación o el cambio de nombre del origen puede ser incierto; verifique antes de reintentar", "O destino criado é preservado. A remoção ou renomeação da origem pode ser incerta; verifique antes de tentar novamente", "Созданное назначение сохранено. Результат удаления или переименования источника может быть неизвестен; проверьте перед повтором", "يتم الاحتفاظ بأي وجهة تم إنشاؤها. قد يكون حذف المصدر أو إعادة تسميته غير مؤكد؛ تحقق قبل إعادة المحاولة", "बनाया गया गंतव्य सुरक्षित है। स्रोत हटाने या नाम बदलने का परिणाम अनिश्चित हो सकता है; फिर प्रयास करने से पहले जाँचें"],
    PdfLoading => ["Loading PDF…", "正在加载 PDF…", "正在載入 PDF…", "PDF を読み込み中…", "PDF 불러오는 중…", "Chargement du PDF…", "PDF wird geladen…", "Cargando PDF…", "Carregando PDF…", "Загрузка PDF…", "جارٍ تحميل PDF…", "PDF लोड हो रहा है…"],
    PdfNoOutline => ["No table of contents", "无目录", "無目錄", "目次なし", "목차 없음", "Aucune table des matières", "Kein Inhaltsverzeichnis", "Sin índice", "Sem sumário", "Нет оглавления", "لا يوجد فهرس", "विषय सूची नहीं है"],
    PdfNoText => ["This PDF has no extractable text", "此 PDF 没有可提取的文字", "此 PDF 沒有可擷取的文字", "抽出できるテキストがありません", "추출할 텍스트가 없습니다", "Ce PDF ne contient aucun texte extractible", "Dieses PDF enthält keinen extrahierbaren Text", "Este PDF no tiene texto extraíble", "Este PDF não tem texto extraível", "В PDF нет извлекаемого текста", "لا يحتوي PDF على نص قابل للاستخراج", "इस PDF में निकालने योग्य पाठ नहीं है"],
    PdfProtected => ["Encrypted PDFs are not supported yet", "暂不支持加密 PDF", "暫不支援加密 PDF", "暗号化 PDF はまだ未対応です", "암호화된 PDF는 아직 지원하지 않습니다", "Les PDF chiffrés ne sont pas encore pris en charge", "Verschlüsselte PDFs werden noch nicht unterstützt", "Los PDF cifrados aún no son compatibles", "PDFs criptografados ainda não são suportados", "Зашифрованные PDF пока не поддерживаются", "ملفات PDF المشفرة غير مدعومة بعد", "एन्क्रिप्टेड PDF अभी समर्थित नहीं हैं"],
    PdfTextLimited => ["Some PDF characters could not be extracted", "部分 PDF 字符无法提取", "部分 PDF 字元無法擷取", "一部の文字を抽出できませんでした", "일부 PDF 문자를 추출할 수 없습니다", "Certains caractères PDF n'ont pas pu être extraits", "Einige PDF-Zeichen konnten nicht extrahiert werden", "No se pudieron extraer algunos caracteres PDF", "Alguns caracteres PDF não puderam ser extraídos", "Некоторые символы PDF не удалось извлечь", "تعذر استخراج بعض أحرف PDF", "कुछ PDF अक्षर निकाले नहीं जा सके"],
    PdfOutlineInvalid => ["PDF outline is invalid or exceeds its limit", "PDF 目录无效或超出限制", "PDF 目錄無效或超出限制", "PDF の目次が無効か上限を超えています", "PDF 목차가 잘못되었거나 제한을 초과했습니다", "Le plan PDF est invalide ou dépasse sa limite", "PDF-Gliederung ungültig oder zu groß", "El índice PDF es inválido o supera su límite", "O sumário PDF é inválido ou excede o limite", "Оглавление PDF повреждено или превышает лимит", "فهرس PDF غير صالح أو يتجاوز الحد", "PDF विषय सूची अमान्य है या सीमा से अधिक है"],
    PdfMemoryLimit => ["PDF exceeds the memory limit", "PDF 超出内存限制", "PDF 超出記憶體限制", "PDF がメモリ上限を超えています", "PDF가 메모리 제한을 초과했습니다", "Le PDF dépasse la limite de mémoire", "PDF überschreitet das Speicherlimit", "El PDF supera el límite de memoria", "O PDF excede o limite de memória", "PDF превышает лимит памяти", "يتجاوز PDF حد الذاكرة", "PDF मेमोरी सीमा से अधिक है"],
    PdfSearchLimit => ["PDF search exceeds 100,000 matches; narrow the query", "PDF 搜索结果超过 100,000 条，请缩小查询范围", "PDF 搜尋結果超過 100,000 筆，請縮小查詢範圍", "検索結果が 100,000 件を超えました。条件を絞ってください", "PDF 검색 결과가 100,000개를 초과했습니다. 검색 범위를 좁히세요", "Plus de 100 000 résultats PDF ; précisez la recherche", "Mehr als 100.000 PDF-Treffer; Suche eingrenzen", "Más de 100.000 coincidencias PDF; acote la búsqueda", "Mais de 100.000 resultados PDF; refine a busca", "Более 100 000 совпадений; уточните запрос", "تجاوز البحث 100,000 نتيجة؛ ضيّق الاستعلام", "PDF में 100,000 से अधिक परिणाम; खोज सीमित करें"],
    PdfUnsupportedDestination => ["Unsupported PDF link destination", "不支持此 PDF 链接目标", "不支援此 PDF 連結目標", "この PDF リンク先は未対応です", "지원하지 않는 PDF 링크 대상", "Destination de lien PDF non prise en charge", "PDF-Linkziel nicht unterstützt", "Destino de enlace PDF no compatible", "Destino de link PDF não suportado", "Неподдерживаемая цель ссылки PDF", "وجهة رابط PDF غير مدعومة", "PDF लिंक लक्ष्य समर्थित नहीं है"],
    DocumentUnavailable => ["Document contents are not available", "文档内容不可用", "文件內容無法使用", "文書の内容を利用できません", "문서 내용을 사용할 수 없습니다", "Le contenu du document est indisponible", "Dokumentinhalt ist nicht verfügbar", "El contenido del documento no está disponible", "O conteúdo do documento não está disponível", "Содержимое документа недоступно", "محتوى المستند غير متاح", "दस्तावेज़ की सामग्री उपलब्ध नहीं है"],
    RecoveryUnavailable => ["This tab has no recoverable contents", "此标签页没有可恢复的内容", "此分頁沒有可還原的內容", "このタブには復元できる内容がありません", "이 탭에는 복구할 수 있는 내용이 없습니다", "Cet onglet n'a aucun contenu récupérable", "Dieser Tab hat keinen wiederherstellbaren Inhalt", "Esta pestaña no tiene contenido recuperable", "Esta aba não tem conteúdo recuperável", "В этой вкладке нет данных для восстановления", "لا تحتوي علامة التبويب على محتوى قابل للاستعادة", "इस टैब में पुनर्प्राप्त करने योग्य सामग्री नहीं है"],
    ExportBusy => ["Another export is still running; wait for it to finish", "另一个导出任务仍在进行中，请等待完成", "另一個匯出工作仍在進行中，請等待完成", "別のエクスポートが実行中です。完了までお待ちください", "다른 내보내기가 진행 중입니다. 완료될 때까지 기다리세요", "Une autre exportation est en cours ; attendez sa fin", "Ein anderer Export läuft noch; bitte warten", "Hay otra exportación en curso; espere a que termine", "Outra exportação está em andamento; aguarde a conclusão", "Другой экспорт ещё выполняется; дождитесь завершения", "هناك عملية تصدير أخرى قيد التشغيل؛ انتظر اكتمالها", "दूसरा निर्यात चल रहा है; उसके पूरा होने तक प्रतीक्षा करें"],
    PngTooLarge => ["Document is too large for a complete PNG export; use PDF or HTML instead", "文档过大，无法完整导出为 PNG，请改用 PDF 或 HTML", "文件過大，無法完整匯出為 PNG，請改用 PDF 或 HTML", "文書が大きすぎて PNG 全体を出力できません。PDF または HTML を使用してください", "문서가 너무 커서 PNG 전체를 내보낼 수 없습니다. PDF 또는 HTML을 사용하세요", "Document trop volumineux pour un PNG complet ; utilisez PDF ou HTML", "Dokument zu groß für vollständigen PNG-Export; PDF oder HTML verwenden", "Documento demasiado grande para PNG completo; use PDF o HTML", "Documento grande demais para PNG completo; use PDF ou HTML", "Документ слишком велик для полного PNG; используйте PDF или HTML", "المستند أكبر من تصدير PNG كامل؛ استخدم PDF أو HTML", "पूरा PNG निर्यात करने के लिए दस्तावेज़ बहुत बड़ा है; PDF या HTML चुनें"],
    FeatureUnavailable => ["This feature is unavailable in this build", "当前构建不支持此功能", "目前組建不支援此功能", "このビルドではこの機能を利用できません", "이 빌드에서는 이 기능을 사용할 수 없습니다", "Cette fonction n'est pas disponible dans cette version", "Diese Funktion ist in diesem Build nicht verfügbar", "Esta función no está disponible en esta compilación", "Este recurso não está disponível nesta versão", "Эта функция недоступна в этой сборке", "هذه الميزة غير متاحة في هذا الإصدار", "यह सुविधा इस बिल्ड में उपलब्ध नहीं है"],
    RenderFailed => ["Could not render the document", "无法渲染文档", "無法呈現文件", "文書を描画できません", "문서를 렌더링할 수 없습니다", "Impossible de rendre le document", "Dokument konnte nicht gerendert werden", "No se pudo renderizar el documento", "Não foi possível renderizar o documento", "Не удалось отрисовать документ", "تعذر عرض المستند", "दस्तावेज़ रेंडर नहीं हो सका"],
    RenderTimedOut => ["Rendering timed out; no output was saved", "渲染超时，未保存输出文件", "呈現逾時，未儲存輸出檔案", "描画がタイムアウトしました。出力は保存されていません", "렌더링 시간이 초과되어 출력이 저장되지 않았습니다", "Délai de rendu dépassé ; aucune sortie enregistrée", "Zeitüberschreitung beim Rendern; keine Ausgabe gespeichert", "Se agotó el tiempo de renderizado; no se guardó salida", "Tempo de renderização esgotado; nenhuma saída salva", "Время отрисовки истекло; результат не сохранён", "انتهت مهلة العرض؛ لم تُحفظ النتيجة", "रेंडरिंग का समय समाप्त; आउटपुट सहेजा नहीं गया"],
    ResourceLoadFailed => ["Could not load export images or fonts", "无法加载导出所需的图片或字体", "無法載入匯出所需的圖片或字型", "エクスポート用の画像またはフォントを読み込めません", "내보낼 이미지 또는 글꼴을 불러올 수 없습니다", "Impossible de charger les images ou polices d'exportation", "Bilder oder Schriften für den Export konnten nicht geladen werden", "No se pudieron cargar imágenes o fuentes de exportación", "Não foi possível carregar imagens ou fontes da exportação", "Не удалось загрузить изображения или шрифты для экспорта", "تعذر تحميل صور التصدير أو خطوطه", "निर्यात की छवियाँ या फ़ॉन्ट लोड नहीं हो सके"],
    InvalidDimensions => ["Invalid image or document dimensions", "图片或文档尺寸无效", "圖片或文件尺寸無效", "画像または文書のサイズが無効です", "이미지 또는 문서 크기가 잘못되었습니다", "Dimensions d'image ou de document invalides", "Ungültige Bild- oder Dokumentabmessungen", "Dimensiones de imagen o documento no válidas", "Dimensões de imagem ou documento inválidas", "Недопустимые размеры изображения или документа", "أبعاد الصورة أو المستند غير صالحة", "छवि या दस्तावेज़ के आयाम अमान्य हैं"],
    NativeRendererFailed => ["Could not initialize the native renderer", "无法初始化系统渲染器", "無法初始化系統呈現器", "ネイティブレンダラーを初期化できません", "네이티브 렌더러를 초기화할 수 없습니다", "Impossible d'initialiser le moteur de rendu natif", "Nativer Renderer konnte nicht initialisiert werden", "No se pudo inicializar el renderizador nativo", "Não foi possível inicializar o renderizador nativo", "Не удалось инициализировать системный рендерер", "تعذر تهيئة العارض الأصلي", "नेटिव रेंडरर आरंभ नहीं हो सका"],
    WebView2Required => ["PDF/PNG export requires Microsoft Edge WebView2 Runtime", "导出 PDF/PNG 需要 Microsoft Edge WebView2 Runtime", "匯出 PDF/PNG 需要 Microsoft Edge WebView2 Runtime", "PDF/PNG エクスポートには Microsoft Edge WebView2 Runtime が必要です", "PDF/PNG 내보내기에는 Microsoft Edge WebView2 Runtime이 필요합니다", "L'export PDF/PNG nécessite Microsoft Edge WebView2 Runtime", "PDF/PNG-Export benötigt Microsoft Edge WebView2 Runtime", "La exportación PDF/PNG requiere Microsoft Edge WebView2 Runtime", "A exportação PDF/PNG requer Microsoft Edge WebView2 Runtime", "Для экспорта PDF/PNG требуется Microsoft Edge WebView2 Runtime", "يتطلب تصدير PDF/PNG برنامج Microsoft Edge WebView2 Runtime", "PDF/PNG निर्यात के लिए Microsoft Edge WebView2 Runtime आवश्यक है"],
    WebKitRequired => ["PDF/PNG export requires WebKitGTK; install libwebkit2gtk-4.1-0 or webkit2gtk4.1", "导出 PDF/PNG 需要 WebKitGTK，请安装 libwebkit2gtk-4.1-0 或 webkit2gtk4.1", "匯出 PDF/PNG 需要 WebKitGTK，請安裝 libwebkit2gtk-4.1-0 或 webkit2gtk4.1", "PDF/PNG エクスポートには WebKitGTK が必要です。libwebkit2gtk-4.1-0 または webkit2gtk4.1 をインストールしてください", "PDF/PNG 내보내기에는 WebKitGTK가 필요합니다. libwebkit2gtk-4.1-0 또는 webkit2gtk4.1을 설치하세요", "L'export PDF/PNG nécessite WebKitGTK ; installez libwebkit2gtk-4.1-0 ou webkit2gtk4.1", "PDF/PNG-Export benötigt WebKitGTK; libwebkit2gtk-4.1-0 oder webkit2gtk4.1 installieren", "PDF/PNG requiere WebKitGTK; instale libwebkit2gtk-4.1-0 o webkit2gtk4.1", "PDF/PNG requer WebKitGTK; instale libwebkit2gtk-4.1-0 ou webkit2gtk4.1", "Для PDF/PNG нужен WebKitGTK; установите libwebkit2gtk-4.1-0 или webkit2gtk4.1", "يتطلب PDF/PNG ‏WebKitGTK؛ ثبّت libwebkit2gtk-4.1-0 أو webkit2gtk4.1", "PDF/PNG के लिए WebKitGTK चाहिए; libwebkit2gtk-4.1-0 या webkit2gtk4.1 स्थापित करें"],
    DesktopRequired => ["Export requires an available Wayland/X11 desktop", "导出需要可用的 Wayland/X11 桌面", "匯出需要可用的 Wayland/X11 桌面", "エクスポートには使用可能な Wayland/X11 デスクトップが必要です", "내보내기에는 사용 가능한 Wayland/X11 데스크톱이 필요합니다", "L'export nécessite un bureau Wayland/X11 disponible", "Export benötigt eine verfügbare Wayland/X11-Oberfläche", "La exportación requiere un escritorio Wayland/X11 disponible", "A exportação requer um ambiente Wayland/X11 disponível", "Для экспорта нужен доступный рабочий стол Wayland/X11", "يتطلب التصدير سطح مكتب Wayland/X11 متاحًا", "निर्यात के लिए उपलब्ध Wayland/X11 डेस्कटॉप चाहिए"],
    InvalidOutput => ["The renderer returned empty or invalid output", "渲染器返回了空白或无效结果", "呈現器傳回空白或無效結果", "レンダラーが空または無効な結果を返しました", "렌더러가 비어 있거나 잘못된 결과를 반환했습니다", "Le moteur a renvoyé un résultat vide ou invalide", "Renderer lieferte eine leere oder ungültige Ausgabe", "El renderizador devolvió una salida vacía o no válida", "O renderizador retornou saída vazia ou inválida", "Рендерер вернул пустой или неверный результат", "أعاد العارض نتيجة فارغة أو غير صالحة", "रेंडरर ने खाली या अमान्य आउटपुट दिया"],
    MeasureFailed => ["Could not measure the complete document layout", "无法测量完整文档布局", "無法測量完整文件配置", "文書全体のレイアウトを測定できません", "전체 문서 레이아웃을 측정할 수 없습니다", "Impossible de mesurer la mise en page complète", "Vollständiges Dokumentlayout konnte nicht vermessen werden", "No se pudo medir el diseño completo del documento", "Não foi possível medir o layout completo do documento", "Не удалось измерить полный макет документа", "تعذر قياس تخطيط المستند بالكامل", "संपूर्ण दस्तावेज़ लेआउट मापा नहीं जा सका"],
    InvalidRequest => ["Invalid application request", "应用请求无效", "應用程式請求無效", "アプリケーションの要求が無効です", "잘못된 애플리케이션 요청", "Requête d'application invalide", "Ungültige Anwendungsanfrage", "Solicitud de aplicación no válida", "Solicitação do aplicativo inválida", "Недопустимый запрос приложения", "طلب تطبيق غير صالح", "अमान्य ऐप अनुरोध"],
    TempDirectory => ["Could not create temporary export files", "无法创建临时导出文件", "無法建立暫存匯出檔案", "一時エクスポートファイルを作成できません", "임시 내보내기 파일을 만들 수 없습니다", "Impossible de créer les fichiers temporaires d'export", "Temporäre Exportdateien konnten nicht erstellt werden", "No se pudieron crear archivos temporales de exportación", "Não foi possível criar arquivos temporários de exportação", "Не удалось создать временные файлы экспорта", "تعذر إنشاء ملفات تصدير مؤقتة", "अस्थायी निर्यात फ़ाइलें नहीं बन सकीं"],
    ExportProcess => ["Native export process failed", "系统导出进程失败", "系統匯出程序失敗", "ネイティブエクスポートプロセスが失敗しました", "네이티브 내보내기 프로세스 실패", "Échec du processus d'export natif", "Nativer Exportprozess fehlgeschlagen", "Falló el proceso de exportación nativo", "Falha no processo de exportação nativo", "Ошибка системного процесса экспорта", "فشلت عملية التصدير الأصلية", "नेटिव निर्यात प्रक्रिया विफल"],
    ImageRead => ["Could not read the image", "无法读取图片", "無法讀取圖片", "画像を読み取れません", "이미지를 읽을 수 없습니다", "Impossible de lire l'image", "Bild konnte nicht gelesen werden", "No se pudo leer la imagen", "Não foi possível ler a imagem", "Не удалось прочитать изображение", "تعذرت قراءة الصورة", "छवि पढ़ी नहीं जा सकी"],
    ImageDecode => ["Could not decode the image", "无法解码图片", "無法解碼圖片", "画像をデコードできません", "이미지를 디코딩할 수 없습니다", "Impossible de décoder l'image", "Bild konnte nicht dekodiert werden", "No se pudo decodificar la imagen", "Não foi possível decodificar a imagem", "Не удалось декодировать изображение", "تعذر فك ترميز الصورة", "छवि डिकोड नहीं हो सकी"],
    ImageTooLarge => ["Image exceeds the size or memory limit", "图片超出大小或内存限制", "圖片超出大小或記憶體限制", "画像がサイズまたはメモリの上限を超えています", "이미지가 크기 또는 메모리 제한을 초과했습니다", "L'image dépasse la limite de taille ou de mémoire", "Bild überschreitet Größen- oder Speicherlimit", "La imagen supera el límite de tamaño o memoria", "A imagem excede o limite de tamanho ou memória", "Изображение превышает ограничение размера или памяти", "تتجاوز الصورة حد الحجم أو الذاكرة", "छवि आकार या मेमोरी सीमा से अधिक है"],
    ImagesTooLarge => ["Export images exceed the document size limit", "导出图片总量超出文档大小限制", "匯出圖片總量超出文件大小限制", "エクスポート画像が文書のサイズ制限を超えています", "내보낼 이미지가 문서 크기 제한을 초과했습니다", "Les images exportées dépassent la limite du document", "Exportbilder überschreiten das Dokumentgrößenlimit", "Las imágenes de exportación superan el límite del documento", "As imagens de exportação excedem o limite do documento", "Экспортируемые изображения превышают лимит документа", "تتجاوز صور التصدير حد حجم المستند", "निर्यात छवियाँ दस्तावेज़ आकार सीमा से अधिक हैं"],
    FileMissing => ["File or link target was not found", "未找到文件或链接目标", "找不到檔案或連結目標", "ファイルまたはリンク先が見つかりません", "파일 또는 링크 대상을 찾을 수 없습니다", "Fichier ou cible du lien introuvable", "Datei oder Linkziel nicht gefunden", "No se encontró el archivo o destino del enlace", "Arquivo ou destino do link não encontrado", "Файл или цель ссылки не найдены", "لم يُعثر على الملف أو هدف الرابط", "फ़ाइल या लिंक लक्ष्य नहीं मिला"],
    WorkspaceRootProtected => ["Remove the workspace before renaming, moving or deleting its root folder", "请先移除工作区，再重命名、移动或删除其根目录", "請先移除工作區，再重新命名、移動或刪除其根目錄", "ルートフォルダーを変更する前にワークスペースを削除してください", "루트 폴더를 변경하기 전에 작업 영역을 제거하세요", "Retirez l’espace de travail avant de modifier son dossier racine", "Entfernen Sie den Arbeitsbereich, bevor Sie seinen Stammordner ändern", "Quite el espacio de trabajo antes de modificar su carpeta raíz", "Remova o espaço de trabalho antes de modificar a pasta raiz", "Удалите рабочую область перед изменением её корневой папки", "أزل مساحة العمل قبل تغيير المجلد الجذر", "रूट फ़ोल्डर बदलने से पहले कार्यक्षेत्र हटाएँ"],
    InvalidPath => ["Invalid file path or file URL", "文件路径或文件 URL 无效", "檔案路徑或檔案 URL 無效", "ファイルパスまたはファイル URL が無効です", "파일 경로 또는 파일 URL이 잘못되었습니다", "Chemin ou URL de fichier invalide", "Ungültiger Dateipfad oder Datei-URL", "Ruta o URL de archivo no válida", "Caminho ou URL de arquivo inválido", "Недопустимый путь или URL файла", "مسار الملف أو عنوانه غير صالح", "फ़ाइल पथ या URL अमान्य है"],
    HomeUnavailable => ["Home directory is unavailable", "用户目录不可用", "使用者目錄無法使用", "ホームディレクトリを利用できません", "홈 디렉터리를 사용할 수 없습니다", "Dossier personnel indisponible", "Benutzerverzeichnis nicht verfügbar", "Carpeta personal no disponible", "Pasta pessoal indisponível", "Домашняя папка недоступна", "المجلد الرئيسي غير متاح", "होम फ़ोल्डर उपलब्ध नहीं है"],
    ReadFile => ["Read file", "读取文件", "讀取檔案", "ファイルを読み込む", "파일 읽기", "Lire le fichier", "Datei lesen", "Leer archivo", "Ler arquivo", "Чтение файла", "قراءة الملف", "फ़ाइल पढ़ना"],
    WriteFile => ["Write file", "写入文件", "寫入檔案", "ファイルを書き込む", "파일 쓰기", "Écrire le fichier", "Datei schreiben", "Escribir archivo", "Gravar arquivo", "Запись файла", "كتابة الملف", "फ़ाइल लिखना"],
    InspectPath => ["Inspect path", "检查路径", "檢查路徑", "パスを調べる", "경로 검사", "Examiner le chemin", "Pfad prüfen", "Examinar ruta", "Inspecionar caminho", "Проверка пути", "فحص المسار", "पथ जाँचना"],
    ReadDirectory => ["Read directory", "读取目录", "讀取目錄", "ディレクトリを読み込む", "디렉터리 읽기", "Lire le dossier", "Verzeichnis lesen", "Leer directorio", "Ler diretório", "Чтение каталога", "قراءة المجلد", "फ़ोल्डर पढ़ना"],
    CreateDirectory => ["Create directory", "创建目录", "建立目錄", "ディレクトリを作成する", "디렉터리 만들기", "Créer un dossier", "Verzeichnis erstellen", "Crear directorio", "Criar diretório", "Создание каталога", "إنشاء مجلد", "फ़ोल्डर बनाना"],
    CopyFile => ["Copy file", "复制文件", "複製檔案", "ファイルをコピーする", "파일 복사", "Copier le fichier", "Datei kopieren", "Copiar archivo", "Copiar arquivo", "Копирование файла", "نسخ الملف", "फ़ाइल कॉपी करना"],
    MoveFile => ["Move or rename file", "移动或重命名文件", "移動或重新命名檔案", "ファイルの移動または名前変更", "파일 이동 또는 이름 변경", "Déplacer ou renommer le fichier", "Datei verschieben oder umbenennen", "Mover o renombrar archivo", "Mover ou renomear arquivo", "Перемещение или переименование файла", "نقل الملف أو إعادة تسميته", "फ़ाइल स्थानांतरित करना या नाम बदलना"],
    DeleteFile => ["Delete file", "删除文件", "刪除檔案", "ファイルを削除する", "파일 삭제", "Supprimer le fichier", "Datei löschen", "Eliminar archivo", "Excluir arquivo", "Удаление файла", "حذف الملف", "फ़ाइल हटाना"],
    SaveSettings => ["Save settings", "保存设置", "儲存設定", "設定を保存する", "설정 저장", "Enregistrer les réglages", "Einstellungen speichern", "Guardar ajustes", "Salvar configurações", "Сохранение настроек", "حفظ الإعدادات", "सेटिंग सहेजना"],
    SearchFiles => ["Search workspace files", "搜索工作区文件", "搜尋工作區檔案", "ワークスペースのファイルを検索", "작업 공간 파일 검색", "Rechercher les fichiers de l'espace", "Arbeitsbereichsdateien durchsuchen", "Buscar archivos del espacio de trabajo", "Pesquisar arquivos do espaço de trabalho", "Поиск файлов рабочей области", "البحث في ملفات مساحة العمل", "कार्यक्षेत्र फ़ाइलें खोजना"],
    OpenFile => ["Open file", "打开文件", "開啟檔案", "ファイルを開く", "파일 열기", "Ouvrir le fichier", "Datei öffnen", "Abrir archivo", "Abrir arquivo", "Открытие файла", "فتح الملف", "फ़ाइल खोलना"],
    FileAction => ["File operation", "文件操作", "檔案操作", "ファイル操作", "파일 작업", "Opération sur les fichiers", "Dateioperation", "Operación de archivo", "Operação de arquivo", "Файловая операция", "عملية ملف", "फ़ाइल कार्य"],
    TaskBusy => ["A previous operation is still running; no duplicate was started", "上一个操作仍在运行，未启动重复任务", "上一個操作仍在執行，未啟動重複工作", "前の操作が実行中のため重複した処理は開始されませんでした", "이전 작업이 실행 중이며 중복 작업은 시작되지 않았습니다", "Une opération précédente est en cours ; aucun doublon lancé", "Vorheriger Vorgang läuft noch; kein doppelter Vorgang gestartet", "La operación anterior sigue en curso; no se inició otra", "A operação anterior continua em andamento; nenhuma duplicata iniciada", "Предыдущая операция ещё выполняется; повторная не запущена", "العملية السابقة مستمرة؛ لم تبدأ عملية مكررة", "पिछला कार्य चल रहा है; दोबारा कार्य शुरू नहीं हुआ"],
    TaskNotStarted => ["The filesystem task could not start; try again later", "文件系统任务未能启动，请稍后重试", "檔案系統工作未能啟動，請稍後重試", "ファイルシステム処理を開始できませんでした。後で再試行してください", "파일 시스템 작업을 시작하지 못했습니다. 나중에 다시 시도하세요", "La tâche de fichiers n'a pas démarré ; réessayez plus tard", "Dateisystemvorgang konnte nicht starten; später erneut versuchen", "No se pudo iniciar la tarea de archivos; reintente más tarde", "A tarefa de arquivos não iniciou; tente mais tarde", "Не удалось запустить файловую задачу; повторите позже", "تعذر بدء مهمة الملفات؛ حاول لاحقًا", "फ़ाइल कार्य शुरू नहीं हो सका; बाद में प्रयास करें"],
    TaskTimedOut => ["The operation timed out and may still be running; its outcome is not yet known", "操作等待超时，但可能仍在运行，结果尚不确定", "操作等待逾時，但可能仍在執行，結果尚不確定", "操作の待機がタイムアウトしました。まだ実行中の可能性があり、結果は未確定です", "작업 대기 시간이 초과되었습니다. 계속 실행 중일 수 있으며 결과는 아직 알 수 없습니다", "Délai dépassé ; l'opération peut continuer et son résultat est inconnu", "Zeitüberschreitung; Vorgang läuft möglicherweise weiter, Ergebnis unbekannt", "Se agotó el tiempo; la operación puede seguir y el resultado es incierto", "Tempo esgotado; a operação pode continuar e o resultado é incerto", "Время ожидания истекло; операция может продолжаться, результат неизвестен", "انتهت المهلة وقد تستمر العملية؛ نتيجتها غير معروفة بعد", "प्रतीक्षा समय समाप्त; कार्य अभी चल सकता है और परिणाम अज्ञात है"],
    WorkerFailed => ["Background worker failed", "后台工作线程失败", "背景工作執行緒失敗", "バックグラウンド処理が失敗しました", "백그라운드 작업 실패", "Échec de la tâche en arrière-plan", "Hintergrundprozess fehlgeschlagen", "Falló la tarea en segundo plano", "Falha na tarefa em segundo plano", "Ошибка фонового обработчика", "فشل العامل في الخلفية", "पृष्ठभूमि कार्य विफल"],
    AutoSavePaused => ["Automatic saves are paused until an explicit save succeeds; a previous write may be incomplete", "自动保存已暂停，需手动保存成功后恢复；上次写入可能尚未完成", "自動儲存已暫停，需手動儲存成功後恢復；上次寫入可能尚未完成", "明示的な保存が成功するまで自動保存は停止します。以前の書き込みが未完了の可能性があります", "수동 저장이 성공할 때까지 자동 저장이 중지됩니다. 이전 쓰기가 완료되지 않았을 수 있습니다", "Sauvegardes automatiques suspendues jusqu'à une sauvegarde explicite réussie ; écriture précédente peut-être incomplète", "Automatisches Speichern pausiert bis zum erfolgreichen manuellen Speichern; vorheriger Schreibvorgang eventuell unvollständig", "Guardado automático pausado hasta un guardado manual correcto; la escritura anterior puede estar incompleta", "Salvamento automático pausado até salvar manualmente com sucesso; a gravação anterior pode estar incompleta", "Автосохранение приостановлено до успешного ручного сохранения; предыдущая запись может быть неполной", "الحفظ التلقائي متوقف حتى نجاح حفظ يدوي؛ قد تكون الكتابة السابقة غير مكتملة", "सफल मैनुअल सेव तक स्वतः सहेजना रुका है; पिछला लेखन अधूरा हो सकता है"],
    SavePending => ["A previous save is still running; wait until its outcome is known before saving again", "上次保存仍在运行，请等待结果确定后再保存", "上次儲存仍在執行，請等待結果確定後再儲存", "前の保存が実行中です。結果が確定してから再保存してください", "이전 저장이 진행 중입니다. 결과가 확인된 후 다시 저장하세요", "Une sauvegarde est en cours ; attendez son résultat avant de recommencer", "Vorherige Speicherung läuft noch; Ergebnis vor erneutem Speichern abwarten", "El guardado anterior sigue en curso; espere su resultado antes de guardar de nuevo", "O salvamento anterior continua; aguarde o resultado antes de salvar novamente", "Предыдущее сохранение ещё выполняется; дождитесь результата", "الحفظ السابق مستمر؛ انتظر نتيجته قبل الحفظ مجددًا", "पिछला सेव चल रहा है; दोबारा सहेजने से पहले परिणाम की प्रतीक्षा करें"],
    WriteFallbackFailed => ["Atomic write and direct-write fallback both failed", "原子写入和直接写入回退均失败", "原子寫入和直接寫入備援均失敗", "アトミック書き込みと直接書き込みの両方が失敗しました", "원자적 쓰기와 직접 쓰기 모두 실패했습니다", "L'écriture atomique et l'écriture directe ont échoué", "Atomares Schreiben und direktes Ersatzschreiben fehlgeschlagen", "Fallaron la escritura atómica y la alternativa directa", "Falharam a gravação atômica e a alternativa direta", "Атомарная и прямая запись завершились ошибкой", "فشلت الكتابة الذرية والكتابة المباشرة البديلة", "एटॉमिक और वैकल्पिक सीधे लेखन दोनों विफल"],
    RecoveryRetained => ["Move failed; the destination is retained for recovery and the source was not fully removed", "移动失败；目标副本已保留以便恢复，源文件未完全移除", "移動失敗；目標副本已保留以便復原，來源檔案未完全移除", "移動に失敗しました。復元用の移動先を保持し、移動元は完全には削除されていません", "이동 실패. 복구용 대상이 보존되었으며 원본이 완전히 제거되지 않았습니다", "Déplacement échoué ; destination conservée pour récupération, source non entièrement supprimée", "Verschieben fehlgeschlagen; Ziel zur Wiederherstellung behalten, Quelle nicht vollständig entfernt", "Falló el movimiento; se conserva el destino para recuperar y el origen no se eliminó por completo", "Movimentação falhou; destino mantido para recuperação, origem não removida por completo", "Перемещение не удалось; копия сохранена для восстановления, источник удалён не полностью", "فشل النقل؛ الوجهة محفوظة للاسترداد ولم يُحذف المصدر بالكامل", "स्थानांतरण विफल; पुनर्प्राप्ति के लिए लक्ष्य रखा गया और स्रोत पूरी तरह नहीं हटाया गया"],
    MoveFallbackFailed => ["Rename and copy fallback failed", "重命名和复制回退失败", "重新命名和複製備援失敗", "名前変更とコピーの代替処理が失敗しました", "이름 변경 및 복사 대체 작업 실패", "Échec du renommage et de la copie de secours", "Umbenennen und Ersatzkopieren fehlgeschlagen", "Fallaron el renombrado y la copia alternativa", "Falharam a renomeação e a cópia alternativa", "Переименование и резервное копирование не удались", "فشلت إعادة التسمية والنسخ البديل", "नाम बदलना और वैकल्पिक कॉपी विफल"],
    CleanupFailed => ["Could not clean up the incomplete destination", "无法清理未完成的目标副本", "無法清理未完成的目標副本", "未完了の移動先を削除できません", "불완전한 대상을 정리할 수 없습니다", "Impossible de nettoyer la destination incomplète", "Unvollständiges Ziel konnte nicht bereinigt werden", "No se pudo limpiar el destino incompleto", "Não foi possível limpar o destino incompleto", "Не удалось удалить неполную копию", "تعذر تنظيف الوجهة غير المكتملة", "अधूरा लक्ष्य साफ़ नहीं किया जा सका"],
    NotDirectory => ["The selected path is not a directory", "所选路径不是目录", "所選路徑不是目錄", "選択したパスはディレクトリではありません", "선택한 경로가 디렉터리가 아닙니다", "Le chemin sélectionné n'est pas un dossier", "Ausgewählter Pfad ist kein Verzeichnis", "La ruta seleccionada no es un directorio", "O caminho selecionado não é um diretório", "Выбранный путь не является каталогом", "المسار المحدد ليس مجلدًا", "चुना गया पथ फ़ोल्डर नहीं है"],
    NotFile => ["The selected path is not a file", "所选路径不是文件", "所選路徑不是檔案", "選択したパスはファイルではありません", "선택한 경로가 파일이 아닙니다", "Le chemin sélectionné n'est pas un fichier", "Ausgewählter Pfad ist keine Datei", "La ruta seleccionada no es un archivo", "O caminho selecionado não é um arquivo", "Выбранный путь не является файлом", "المسار المحدد ليس ملفًا", "चुना गया पथ फ़ाइल नहीं है"],
    ParentMissing => ["The file has no parent directory", "文件没有父目录", "檔案沒有上層目錄", "ファイルに親ディレクトリがありません", "파일에 상위 디렉터리가 없습니다", "Le fichier n'a pas de dossier parent", "Datei hat kein übergeordnetes Verzeichnis", "El archivo no tiene directorio padre", "O arquivo não tem diretório pai", "У файла нет родительского каталога", "ليس للملف مجلد أصل", "फ़ाइल का मूल फ़ोल्डर नहीं है"],
    AbsolutePathRequired => ["An absolute path is required", "需要绝对路径", "需要絕對路徑", "絶対パスが必要です", "절대 경로가 필요합니다", "Un chemin absolu est requis", "Absoluter Pfad erforderlich", "Se requiere una ruta absoluta", "É necessário um caminho absoluto", "Требуется абсолютный путь", "مطلوب مسار مطلق", "पूर्ण पथ आवश्यक है"],
    OutsideWorkspace => ["The file is outside the current workspace", "文件位于当前工作区之外", "檔案位於目前工作區之外", "ファイルは現在のワークスペース外にあります", "파일이 현재 작업 공간 밖에 있습니다", "Le fichier est hors de l'espace de travail actuel", "Datei liegt außerhalb des aktuellen Arbeitsbereichs", "El archivo está fuera del espacio de trabajo actual", "O arquivo está fora do espaço de trabalho atual", "Файл находится вне текущей рабочей области", "الملف خارج مساحة العمل الحالية", "फ़ाइल वर्तमान कार्यक्षेत्र से बाहर है"],
    AttachmentOutsideBase => ["Attachment directory escapes the selected base folder", "附件目录超出所选基础目录", "附件目錄超出所選基礎目錄", "添付フォルダーが選択した基準フォルダー外にあります", "첨부 디렉터리가 선택한 기본 폴더를 벗어납니다", "Le dossier des pièces jointes dépasse le dossier de base", "Anhangsverzeichnis liegt außerhalb des Basisordners", "El directorio de adjuntos sale de la carpeta base", "O diretório de anexos está fora da pasta base", "Каталог вложений выходит за пределы базовой папки", "مجلد المرفقات خارج المجلد الأساسي المحدد", "संलग्नक फ़ोल्डर चुने गए आधार फ़ोल्डर से बाहर है"],
    FolderEmpty => ["Attachment folder cannot be empty", "附件目录不能为空", "附件目錄不能為空", "添付フォルダーを空にできません", "첨부 폴더는 비워 둘 수 없습니다", "Le dossier des pièces jointes ne peut pas être vide", "Anhangsordner darf nicht leer sein", "La carpeta de adjuntos no puede estar vacía", "A pasta de anexos não pode estar vazia", "Папка вложений не может быть пустой", "لا يمكن أن يكون مجلد المرفقات فارغًا", "संलग्नक फ़ोल्डर खाली नहीं हो सकता"],
    RelativeFolderRequired => ["Attachment folder must be a relative path without '..'", "附件目录必须是相对路径且不能包含“..”", "附件目錄必須是相對路徑且不能包含「..」", "添付フォルダーには「..」を含まない相対パスが必要です", "첨부 폴더는 '..' 없는 상대 경로여야 합니다", "Le dossier des pièces jointes doit être un chemin relatif sans '..'", "Anhangsordner muss ein relativer Pfad ohne '..' sein", "La carpeta de adjuntos debe ser relativa y sin '..'", "A pasta de anexos deve ser um caminho relativo sem '..'", "Путь к вложениям должен быть относительным без '..'", "يجب أن يكون مجلد المرفقات مسارًا نسبيًا دون '..'", "संलग्नक फ़ोल्डर '..' रहित सापेक्ष पथ होना चाहिए"],
    UnsafeDelete => ["Cannot delete a filesystem root, current directory, parent path, or empty path", "不能删除文件系统根目录、当前目录、父路径或空路径", "不能刪除檔案系統根目錄、目前目錄、上層路徑或空路徑", "ルート、現在のディレクトリ、親パス、空のパスは削除できません", "루트, 현재 디렉터리, 상위 경로 또는 빈 경로는 삭제할 수 없습니다", "Impossible de supprimer la racine, le dossier actuel, un parent ou un chemin vide", "Wurzelverzeichnis, aktuelles Verzeichnis, Elternpfad oder leerer Pfad können nicht gelöscht werden", "No se puede eliminar la raíz, el directorio actual, padre o una ruta vacía", "Não é possível excluir raiz, diretório atual, pai ou caminho vazio", "Нельзя удалить корень, текущий каталог, родительский или пустой путь", "لا يمكن حذف الجذر أو المجلد الحالي أو الأصل أو مسار فارغ", "रूट, वर्तमान फ़ोल्डर, मूल पथ या खाली पथ नहीं हटाया जा सकता"],
    VolumeProbe => ["Could not determine the volume type", "无法确定卷类型", "無法確定磁碟區類型", "ボリュームの種類を判定できません", "볼륨 유형을 확인할 수 없습니다", "Impossible de déterminer le type de volume", "Datenträgertyp konnte nicht bestimmt werden", "No se pudo determinar el tipo de volumen", "Não foi possível determinar o tipo do volume", "Не удалось определить тип тома", "تعذر تحديد نوع وحدة التخزين", "वॉल्यूम प्रकार निर्धारित नहीं हो सका"],
    TrashFailed => ["Could not move the item to Trash", "无法将项目移至废纸篓或回收站", "無法將項目移至垃圾桶或資源回收筒", "項目をゴミ箱に移動できません", "항목을 휴지통으로 옮길 수 없습니다", "Impossible de déplacer l'élément vers la corbeille", "Element konnte nicht in den Papierkorb verschoben werden", "No se pudo mover el elemento a la papelera", "Não foi possível mover o item para a lixeira", "Не удалось переместить объект в корзину", "تعذر نقل العنصر إلى سلة المحذوفات", "आइटम कूड़ेदान में नहीं ले जाया जा सका"],
    NetworkDeleteFailed => ["Permanent deletion on the network volume failed", "网络卷上的永久删除失败", "網路磁碟區上的永久刪除失敗", "ネットワークボリュームでの完全削除に失敗しました", "네트워크 볼륨에서 영구 삭제 실패", "Échec de la suppression définitive sur le volume réseau", "Endgültiges Löschen auf dem Netzlaufwerk fehlgeschlagen", "Falló la eliminación permanente en el volumen de red", "Falha na exclusão permanente no volume de rede", "Не удалось окончательно удалить с сетевого тома", "فشل الحذف الدائم على وحدة التخزين الشبكية", "नेटवर्क वॉल्यूम पर स्थायी हटाना विफल"],
    ClipboardFailed => ["Could not access or write the system clipboard", "无法访问或写入系统剪贴板", "無法存取或寫入系統剪貼簿", "システムクリップボードへのアクセスまたは書き込みができません", "시스템 클립보드에 접근하거나 쓸 수 없습니다", "Impossible d'accéder au presse-papiers ou d'y écrire", "Systemzwischenablage konnte nicht geöffnet oder beschrieben werden", "No se pudo acceder o escribir en el portapapeles", "Não foi possível acessar ou gravar na área de transferência", "Не удалось открыть буфер обмена или записать в него", "تعذر الوصول إلى حافظة النظام أو الكتابة فيها", "सिस्टम क्लिपबोर्ड का उपयोग या लेखन नहीं हो सका"],
    HelperUnavailable => ["Required system helper is unavailable; install it and try again", "所需的系统工具不可用，请安装后重试", "所需的系統工具無法使用，請安裝後重試", "必要なシステムツールがありません。インストールして再試行してください", "필요한 시스템 도구가 없습니다. 설치 후 다시 시도하세요", "Outil système requis indisponible ; installez-le puis réessayez", "Benötigtes Systemwerkzeug fehlt; installieren und erneut versuchen", "No está disponible la herramienta requerida; instálela y reintente", "Ferramenta necessária indisponível; instale e tente novamente", "Нужная системная программа недоступна; установите её и повторите", "أداة النظام المطلوبة غير متاحة؛ ثبّتها وحاول مجددًا", "आवश्यक सिस्टम टूल उपलब्ध नहीं; स्थापित कर फिर प्रयास करें"],
    UnsafeLink => ["Refused to open an unsafe link scheme", "已拒绝打开不安全的链接协议", "已拒絕開啟不安全的連結協定", "安全でないリンクスキームを拒否しました", "안전하지 않은 링크 스킴 열기를 거부했습니다", "Ouverture d'un protocole de lien non sûr refusée", "Unsicheres Linkprotokoll wurde abgelehnt", "Se rechazó abrir un esquema de enlace inseguro", "Abertura de protocolo de link inseguro recusada", "Открытие небезопасной схемы ссылки отклонено", "رُفض فتح بروتوكول رابط غير آمن", "असुरक्षित लिंक योजना खोलने से मना किया गया"],
    SystemOpenFailed => ["The system application could not open the file or link", "系统应用无法打开文件或链接", "系統應用程式無法開啟檔案或連結", "システムアプリでファイルまたはリンクを開けません", "시스템 앱이 파일 또는 링크를 열 수 없습니다", "L'application système n'a pas pu ouvrir le fichier ou le lien", "Systemanwendung konnte Datei oder Link nicht öffnen", "La aplicación del sistema no pudo abrir el archivo o enlace", "O aplicativo do sistema não pôde abrir o arquivo ou link", "Системное приложение не смогло открыть файл или ссылку", "تعذر على تطبيق النظام فتح الملف أو الرابط", "सिस्टम ऐप फ़ाइल या लिंक नहीं खोल सका"],
    TerminalUnavailable => ["Terminal is unavailable or not running", "终端不可用或未运行", "終端機無法使用或未執行", "ターミナルを利用できないか実行されていません", "터미널을 사용할 수 없거나 실행 중이 아닙니다", "Terminal indisponible ou arrêté", "Terminal nicht verfügbar oder nicht gestartet", "Terminal no disponible o detenido", "Terminal indisponível ou parado", "Терминал недоступен или не запущен", "الطرفية غير متاحة أو لا تعمل", "टर्मिनल उपलब्ध नहीं या चल नहीं रहा"],
    TerminalWriteFailed => ["Could not send input to the terminal", "无法向终端发送输入", "無法向終端機傳送輸入", "ターミナルに入力を送信できません", "터미널에 입력을 보낼 수 없습니다", "Impossible d'envoyer la saisie au terminal", "Eingabe konnte nicht an das Terminal gesendet werden", "No se pudo enviar entrada al terminal", "Não foi possível enviar entrada ao terminal", "Не удалось отправить ввод в терминал", "تعذر إرسال الإدخال إلى الطرفية", "टर्मिनल को इनपुट नहीं भेजा जा सका"],
    InvalidHistory => ["Undo history is invalid or incomplete", "撤销历史无效或不完整", "復原歷程無效或不完整", "取り消し履歴が無効または不完全です", "실행 취소 기록이 잘못되었거나 불완전합니다", "Historique d'annulation invalide ou incomplet", "Rückgängig-Verlauf ist ungültig oder unvollständig", "Historial de deshacer no válido o incompleto", "Histórico de desfazer inválido ou incompleto", "История отмены повреждена или неполна", "سجل التراجع غير صالح أو غير مكتمل", "पूर्ववत इतिहास अमान्य या अधूरा है"],
    FileOpenDelivery => ["Could not deliver files to the running application", "无法将文件发送到正在运行的应用", "無法將檔案傳送至執行中的應用程式", "実行中のアプリにファイルを渡せません", "실행 중인 앱에 파일을 전달할 수 없습니다", "Impossible de transmettre les fichiers à l'application ouverte", "Dateien konnten nicht an die laufende Anwendung übergeben werden", "No se pudieron enviar archivos a la aplicación abierta", "Não foi possível enviar arquivos ao aplicativo em execução", "Не удалось передать файлы запущенному приложению", "تعذر إرسال الملفات إلى التطبيق قيد التشغيل", "चल रहे ऐप को फ़ाइलें नहीं भेजी जा सकीं"],
    HeifRequired => ["HEIC/AVIF decoding requires system libheif and HEVC/AV1 decoder plugins", "解码 HEIC/AVIF 需要系统 libheif 和 HEVC/AV1 解码插件", "解碼 HEIC/AVIF 需要系統 libheif 和 HEVC/AV1 解碼外掛", "HEIC/AVIF のデコードには libheif と HEVC/AV1 プラグインが必要です", "HEIC/AVIF 디코딩에는 libheif 및 HEVC/AV1 플러그인이 필요합니다", "Le décodage HEIC/AVIF nécessite libheif et des modules HEVC/AV1", "HEIC/AVIF benötigt libheif und HEVC/AV1-Decoder", "HEIC/AVIF requiere libheif y complementos HEVC/AV1", "HEIC/AVIF requer libheif e plugins HEVC/AV1", "Для HEIC/AVIF нужны libheif и декодеры HEVC/AV1", "يتطلب فك HEIC/AVIF ‏libheif وملحقات HEVC/AV1", "HEIC/AVIF के लिए libheif और HEVC/AV1 डिकोडर चाहिए"],
    WindowsHeifRequired => ["Install HEIF Image Extensions and the required HEVC or AV1 Video Extensions to decode this image", "请安装 HEIF 图像扩展及所需的 HEVC 或 AV1 视频扩展以解码此图片", "請安裝 HEIF 影像延伸模組及所需的 HEVC 或 AV1 視訊延伸模組以解碼此圖片", "この画像には HEIF Image Extensions と HEVC または AV1 Video Extensions が必要です", "이 이미지를 디코딩하려면 HEIF Image Extensions 및 필요한 HEVC 또는 AV1 Video Extensions를 설치하세요", "Installez HEIF Image Extensions et les extensions vidéo HEVC ou AV1 nécessaires", "HEIF Image Extensions und benötigte HEVC- oder AV1-Videoerweiterungen installieren", "Instale HEIF Image Extensions y las extensiones HEVC o AV1 necesarias", "Instale HEIF Image Extensions e extensões HEVC ou AV1 necessárias", "Установите HEIF Image Extensions и необходимые расширения HEVC или AV1", "ثبّت HEIF Image Extensions وامتدادات HEVC أو AV1 المطلوبة", "HEIF Image Extensions और आवश्यक HEVC या AV1 Video Extensions स्थापित करें"],
    InvalidImageData => ["Invalid image data or image URL", "图片数据或图片 URL 无效", "圖片資料或圖片 URL 無效", "画像データまたは画像 URL が無効です", "이미지 데이터 또는 URL이 잘못되었습니다", "Données ou URL d'image invalides", "Ungültige Bilddaten oder Bild-URL", "Datos o URL de imagen no válidos", "Dados ou URL de imagem inválidos", "Недопустимые данные или URL изображения", "بيانات الصورة أو عنوانها غير صالح", "छवि डेटा या URL अमान्य है"],
    DownloadImage => ["Download image", "下载图片", "下載圖片", "画像をダウンロード", "이미지 다운로드", "Télécharger l'image", "Bild herunterladen", "Descargar imagen", "Baixar imagem", "Загрузка изображения", "تنزيل الصورة", "छवि डाउनलोड करना"],
    PrepareExport => ["Prepare document export", "准备文档导出", "準備文件匯出", "文書のエクスポートを準備", "문서 내보내기 준비", "Préparer l'export du document", "Dokumentexport vorbereiten", "Preparar exportación del documento", "Preparar exportação do documento", "Подготовка экспорта документа", "تحضير تصدير المستند", "दस्तावेज़ निर्यात तैयार करना"],
    SaveExport => ["Save exported file", "保存导出文件", "儲存匯出檔案", "エクスポートファイルを保存", "내보낸 파일 저장", "Enregistrer le fichier exporté", "Exportdatei speichern", "Guardar archivo exportado", "Salvar arquivo exportado", "Сохранение экспортированного файла", "حفظ الملف المُصدّر", "निर्यात फ़ाइल सहेजना"],
    ProfileFailed => ["Runtime profiling failed", "运行时性能分析失败", "執行時效能分析失敗", "実行時プロファイリングに失敗しました", "런타임 프로파일링 실패", "Échec du profilage d'exécution", "Laufzeitprofilierung fehlgeschlagen", "Falló el perfil de ejecución", "Falha no perfil de execução", "Ошибка профилирования", "فشل تحليل أداء التشغيل", "रनटाइम प्रोफ़ाइल विफल"],
    InvalidSetting => ["Invalid setting value", "设置值无效", "設定值無效", "設定値が無効です", "설정값이 잘못되었습니다", "Valeur de réglage invalide", "Ungültiger Einstellungswert", "Valor de ajuste no válido", "Valor de configuração inválido", "Недопустимое значение настройки", "قيمة إعداد غير صالحة", "सेटिंग मान अमान्य है"],
    SshHomeUnavailable => ["SSH home directory is unavailable", "SSH 主目录不可用", "SSH 主目錄無法使用", "SSH ホームディレクトリを利用できません", "SSH 홈 디렉터리를 사용할 수 없습니다", "Le dossier personnel SSH est indisponible", "SSH-Home-Verzeichnis ist nicht verfügbar", "El directorio principal SSH no está disponible", "O diretório inicial SSH não está disponível", "Домашний каталог SSH недоступен", "مجلد SSH الرئيسي غير متاح", "SSH होम निर्देशिका उपलब्ध नहीं है"],
    SshConfigIncludeLimit => ["SSH configuration includes exceed the safety limit", "SSH 配置包含项超出安全限制", "SSH 設定包含項目超出安全限制", "SSH 設定の Include が安全上限を超えています", "SSH 설정 Include가 안전 제한을 초과했습니다", "Les inclusions de configuration SSH dépassent la limite", "SSH-Konfigurations-Includes überschreiten das Sicherheitslimit", "Las inclusiones SSH superan el límite de seguridad", "As inclusões SSH excedem o limite de segurança", "Включения конфигурации SSH превышают безопасный предел", "تجاوزت تضمينات إعداد SSH حد الأمان", "SSH कॉन्फ़िगरेशन Include सुरक्षा सीमा से अधिक हैं"],
    SshConfigSizeLimit => ["SSH configuration exceeds the size limit", "SSH 配置超出大小限制", "SSH 設定超出大小限制", "SSH 設定がサイズ上限を超えています", "SSH 설정이 크기 제한을 초과했습니다", "La configuration SSH dépasse la taille maximale", "SSH-Konfiguration überschreitet das Größenlimit", "La configuración SSH supera el límite de tamaño", "A configuração SSH excede o limite de tamanho", "Конфигурация SSH превышает ограничение размера", "تجاوز إعداد SSH حد الحجم", "SSH कॉन्फ़िगरेशन आकार सीमा से अधिक है"],
    SshConfigHostLimit => ["SSH configuration contains too many hosts", "SSH 配置包含过多主机", "SSH 設定包含過多主機", "SSH 設定のホスト数が多すぎます", "SSH 설정에 호스트가 너무 많습니다", "La configuration SSH contient trop d’hôtes", "SSH-Konfiguration enthält zu viele Hosts", "La configuración SSH contiene demasiados hosts", "A configuração SSH contém hosts demais", "В конфигурации SSH слишком много узлов", "يحتوي إعداد SSH على عدد كبير جدًا من المضيفين", "SSH कॉन्फ़िगरेशन में बहुत अधिक होस्ट हैं"],
    SshConfigSyntaxInvalid => ["SSH configuration contains an unterminated quote or escape", "SSH 配置包含未终止的引号或转义", "SSH 設定包含未終止的引號或跳脫字元", "SSH 設定に閉じられていない引用符またはエスケープがあります", "SSH 설정에 닫히지 않은 따옴표 또는 이스케이프가 있습니다", "La configuration SSH contient une citation ou un échappement non terminé", "SSH-Konfiguration enthält ein nicht abgeschlossenes Anführungszeichen oder Escape", "La configuración SSH contiene una comilla o escape sin terminar", "A configuração SSH contém aspas ou escape não terminado", "В конфигурации SSH есть незавершённая кавычка или escape-последовательность", "يحتوي إعداد SSH على اقتباس أو تسلسل هروب غير منتهٍ", "SSH कॉन्फ़िगरेशन में अधूरा उद्धरण या एस्केप है"],
    PreviewFailed => ["Preview could not be rendered", "无法渲染预览", "無法呈現預覽", "プレビューを描画できません", "미리 보기를 렌더링할 수 없습니다", "Impossible de rendre l'aperçu", "Vorschau konnte nicht gerendert werden", "No se pudo renderizar la vista previa", "Não foi possível renderizar a prévia", "Не удалось отрисовать предпросмотр", "تعذر عرض المعاينة", "पूर्वावलोकन रेंडर नहीं हो सका"],
    InvalidShortcut => ["Invalid keyboard shortcut", "键盘快捷键无效", "鍵盤快捷鍵無效", "キーボードショートカットが無効です", "키보드 단축키가 잘못되었습니다", "Raccourci clavier invalide", "Ungültiges Tastenkürzel", "Atajo de teclado no válido", "Atalho de teclado inválido", "Недопустимое сочетание клавиш", "اختصار لوحة المفاتيح غير صالح", "कीबोर्ड शॉर्टकट अमान्य है"],
    DuplicateShortcut => ["Keyboard shortcut is assigned to multiple commands", "键盘快捷键分配给了多个命令", "鍵盤快捷鍵指派給了多個指令", "同じショートカットが複数のコマンドに割り当てられています", "단축키가 여러 명령에 할당되었습니다", "Raccourci affecté à plusieurs commandes", "Tastenkürzel ist mehreren Befehlen zugewiesen", "El atajo está asignado a varios comandos", "O atalho está atribuído a vários comandos", "Сочетание клавиш назначено нескольким командам", "اختصار لوحة المفاتيح مُعين لأوامر متعددة", "शॉर्टकट कई कमांड को दिया गया है"],
    UnknownCommand => ["Unknown command", "未知命令", "未知指令", "不明なコマンド", "알 수 없는 명령", "Commande inconnue", "Unbekannter Befehl", "Comando desconocido", "Comando desconhecido", "Неизвестная команда", "أمر غير معروف", "अज्ञात कमांड"]
}

impl fmt::Display for RuntimeText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text(language()))
    }
}
impl std::error::Error for RuntimeText {}

#[derive(Debug)]
pub(crate) struct RuntimeMessage {
    pub key: RuntimeText,
    pub details: Vec<String>,
}

impl RuntimeMessage {
    pub(crate) fn render(&self, language: UiLanguage) -> String {
        let mut text = self.key.text(language).to_string();
        for detail in &self.details {
            text.push_str(": ");
            text.push_str(detail);
        }
        text
    }
}
impl fmt::Display for RuntimeMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(language()))
    }
}
impl std::error::Error for RuntimeMessage {}

macro_rules! rt {
    ($key:ident $(, $detail:expr)* $(,)?) => {
        $crate::i18n::runtime::RuntimeMessage {
            key: $crate::i18n::runtime::RuntimeText::$key,
            details: vec![$(($detail).to_string()),*],
        }
    };
}
pub(crate) use rt;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_messages_translate_all_languages_and_preserve_details() {
        for key in RuntimeText::ALL {
            for &locale in UiLanguage::supported() {
                assert!(!key.text(locale).is_empty());
                if locale != UiLanguage::English {
                    assert_ne!(
                        key.text(locale),
                        key.text(UiLanguage::English),
                        "{key:?} {locale:?}"
                    );
                }
            }
        }
        let message = rt!(
            FileMissing,
            "/notes/Another export is still running.md",
            "OS error 13"
        );
        let text = message.render(UiLanguage::ChineseSimplified);
        assert!(text.starts_with("未找到"));
        assert!(text.contains("/notes/Another export is still running.md"));
        assert!(text.ends_with("OS error 13"));
        assert_eq!(
            rt!(ExportBusy).render(UiLanguage::ChineseSimplified),
            "另一个导出任务仍在进行中，请等待完成"
        );
    }
}
