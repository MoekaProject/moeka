use super::{UiLanguage, command_label_index};

macro_rules! messages {
    ($($key:ident => $values:expr),+ $(,)?) => {
        #[derive(Clone, Copy, Debug)]
        pub(crate) enum MessageKey { $($key),+ }
        impl MessageKey {
            #[cfg(test)]
            const ALL: &'static [Self] = &[$(Self::$key),+];

            pub(crate) fn text(self, language: UiLanguage) -> &'static str {
                let values: [&str; 12] = match self { $(Self::$key => $values),+ };
                values[command_label_index(language)]
            }
        }
    };
}

messages! {
    EmptyDocumentHint => [
        "Type / for commands or right-click to insert formatting…", "键入 / 命令或右键插入格式…", "輸入 / 指令或按右鍵插入格式…",
        "/ でコマンド、右クリックで書式を挿入…", "/ 명령 또는 오른쪽 클릭으로 서식 삽입…",
        "Tapez / pour les commandes ou faites un clic droit pour insérer un format…",
        "/ für Befehle eingeben oder per Rechtsklick formatieren…", "Escriba / para comandos o haga clic derecho para insertar formato…",
        "Digite / para comandos ou clique com o botão direito para inserir formatação…", "Введите / для команд или нажмите правую кнопку для форматирования…",
        "اكتب / للأوامر أو انقر بالزر الأيمن لإدراج التنسيق…", "कमांड के लिए / लिखें या फ़ॉर्मेट जोड़ने के लिए दायाँ क्लिक करें…"
    ],
    PlainText => ["Plain Text", "纯文本", "純文字", "プレーンテキスト", "일반 텍스트", "Texte brut", "Nur Text", "Texto sin formato", "Texto simples", "Простой текст", "نص عادي", "सादा पाठ"],
    Binary => ["Binary", "二进制", "二進位", "バイナリ", "바이너리", "Binaire", "Binär", "Binario", "Binário", "Двоичный файл", "ملف ثنائي", "बाइनरी"],
    ChooseImage => ["Choose Image", "选择图片", "選擇圖片", "画像を選択", "이미지 선택", "Choisir une image", "Bild auswählen", "Elegir imagen", "Escolher imagem", "Выбрать изображение", "اختيار صورة", "छवि चुनें"],
    AnchorMissing => ["Heading or anchor not found: {anchor}", "未找到标题或锚点：{anchor}", "找不到標題或錨點：{anchor}", "見出しまたはアンカーが見つかりません: {anchor}", "제목 또는 앵커를 찾을 수 없음: {anchor}", "Titre ou ancre introuvable : {anchor}", "Überschrift oder Anker nicht gefunden: {anchor}", "No se encontró el título o ancla: {anchor}", "Título ou âncora não encontrado: {anchor}", "Заголовок или якорь не найден: {anchor}", "لم يُعثر على العنوان أو الرابط الداخلي: {anchor}", "शीर्षक या एंकर नहीं मिला: {anchor}"],
    ChatSourceChanged => ["Cannot add selection to chat: the source document changed.", "无法添加选中内容到聊天：源文档已改变。", "無法將選取內容加入聊天：來源文件已變更。", "選択範囲をチャットに追加できません。元の文書が変更されました。", "선택 항목을 채팅에 추가할 수 없습니다. 원본 문서가 변경되었습니다.", "Impossible d'ajouter la sélection au chat : le document source a changé.", "Auswahl kann nicht zum Chat hinzugefügt werden: Das Quelldokument wurde geändert.", "No se puede añadir la selección al chat: el documento de origen cambió.", "Não foi possível adicionar a seleção ao chat: o documento de origem mudou.", "Нельзя добавить выделение в чат: исходный документ изменился.", "تعذرت إضافة التحديد إلى الدردشة: تغير المستند المصدر.", "चयन चैट में नहीं जोड़ा जा सकता: स्रोत दस्तावेज़ बदल गया।"],
    ChatSourceClosed => ["Cannot add file to chat: the source document was closed or moved.", "无法添加文件到聊天：源文档已关闭或移动。", "無法將檔案加入聊天：來源文件已關閉或移動。", "ファイルをチャットに追加できません。元の文書が閉じられたか移動されました。", "파일을 채팅에 추가할 수 없습니다. 원본 문서가 닫히거나 이동되었습니다.", "Impossible d'ajouter le fichier au chat : le document source a été fermé ou déplacé.", "Datei kann nicht zum Chat hinzugefügt werden: Das Quelldokument wurde geschlossen oder verschoben.", "No se puede añadir el archivo al chat: el documento se cerró o movió.", "Não foi possível adicionar o arquivo ao chat: o documento foi fechado ou movido.", "Нельзя добавить файл в чат: исходный документ закрыт или перемещён.", "تعذرت إضافة الملف إلى الدردشة: أُغلق المستند المصدر أو نُقل.", "फ़ाइल चैट में नहीं जोड़ी जा सकती: स्रोत दस्तावेज़ बंद या स्थानांतरित हुआ।"],
    ChatConflict => ["Cannot add file to chat: unsaved versions conflict: {path}", "无法添加文件到聊天：未保存版本冲突：{path}", "無法將檔案加入聊天：未儲存版本衝突：{path}", "ファイルをチャットに追加できません。未保存の版が競合しています: {path}", "파일을 채팅에 추가할 수 없습니다. 저장되지 않은 버전이 충돌합니다: {path}", "Impossible d'ajouter le fichier au chat : conflit entre versions non enregistrées : {path}", "Datei kann nicht zum Chat hinzugefügt werden: Ungespeicherte Versionen stehen im Konflikt: {path}", "No se puede añadir el archivo al chat: conflicto de versiones sin guardar: {path}", "Não foi possível adicionar o arquivo ao chat: conflito entre versões não salvas: {path}", "Нельзя добавить файл в чат: конфликт несохранённых версий: {path}", "تعذرت إضافة الملف إلى الدردشة: تعارض في النسخ غير المحفوظة: {path}", "फ़ाइल चैट में नहीं जोड़ी जा सकती: सहेजे न गए संस्करणों में टकराव: {path}"],
    ChatTargetChanged => ["Add to chat cancelled: the workspace or target terminal changed.", "已取消添加到聊天：工作区或目标终端已改变。", "已取消加入聊天：工作區或目標終端機已變更。", "チャットへの追加を中止しました。ワークスペースまたは対象ターミナルが変更されました。", "채팅 추가가 취소되었습니다. 작업 공간 또는 대상 터미널이 변경되었습니다.", "Ajout au chat annulé : l'espace de travail ou le terminal cible a changé.", "Zum Chat hinzufügen abgebrochen: Arbeitsbereich oder Zielterminal wurde geändert.", "Se canceló añadir al chat: cambió el espacio de trabajo o terminal de destino.", "Adição ao chat cancelada: o espaço de trabalho ou terminal de destino mudou.", "Добавление в чат отменено: рабочая область или целевой терминал изменились.", "أُلغيت الإضافة إلى الدردشة: تغيرت مساحة العمل أو الطرفية المستهدفة.", "चैट में जोड़ना रद्द: कार्यक्षेत्र या लक्ष्य टर्मिनल बदल गया।"],
    ChatChangedWhileSaving => ["Add to chat cancelled: the document changed while saving. Try again.", "已取消添加到聊天：文档在保存期间改变，请重试。", "已取消加入聊天：文件在儲存期間變更，請重試。", "チャットへの追加を中止しました。保存中に文書が変更されました。再試行してください。", "채팅 추가가 취소되었습니다. 저장 중 문서가 변경되었습니다. 다시 시도하세요.", "Ajout au chat annulé : le document a changé pendant l'enregistrement. Réessayez.", "Zum Chat hinzufügen abgebrochen: Dokument wurde beim Speichern geändert. Erneut versuchen.", "Se canceló añadir al chat: el documento cambió al guardar. Inténtelo de nuevo.", "Adição ao chat cancelada: o documento mudou durante o salvamento. Tente novamente.", "Добавление в чат отменено: документ изменился при сохранении. Повторите попытку.", "أُلغيت الإضافة إلى الدردشة: تغير المستند أثناء الحفظ. أعد المحاولة.", "चैट में जोड़ना रद्द: सहेजते समय दस्तावेज़ बदल गया। फिर प्रयास करें।"],
    ChatUnavailable => ["Chat is unavailable in this build.", "当前版本不支持聊天。", "目前版本不支援聊天。", "このビルドではチャットを利用できません。", "이 빌드에서는 채팅을 사용할 수 없습니다.", "Le chat n'est pas disponible dans cette version.", "Chat ist in dieser Version nicht verfügbar.", "El chat no está disponible en esta versión.", "O chat não está disponível nesta versão.", "Чат недоступен в этой сборке.", "الدردشة غير متاحة في هذا الإصدار.", "इस बिल्ड में चैट उपलब्ध नहीं है।"],
    ChatCancelled => ["Add to chat cancelled", "已取消添加到聊天", "已取消加入聊天", "チャットへの追加を中止しました", "채팅 추가 취소됨", "Ajout au chat annulé", "Zum Chat hinzufügen abgebrochen", "Se canceló añadir al chat", "Adição ao chat cancelada", "Добавление в чат отменено", "أُلغيت الإضافة إلى الدردشة", "चैट में जोड़ना रद्द"],
    ChatSaveSuperseded => ["A newer save replaced this request. Add to chat again after saving finishes.", "本次保存已被更新的请求替代，请等待保存完成后重新添加到聊天。", "此次儲存已被較新的請求取代，請等待儲存完成後重新加入聊天。", "新しい保存要求に置き換えられました。保存完了後に再度チャットに追加してください。", "더 새로운 저장 요청으로 대체되었습니다. 저장 후 채팅에 다시 추가하세요.", "Une sauvegarde plus récente a remplacé cette demande. Réessayez après l'enregistrement.", "Eine neuere Speicherung hat diese Anfrage ersetzt. Nach dem Speichern erneut zum Chat hinzufügen.", "Un guardado más reciente sustituyó esta solicitud. Añada al chat cuando termine.", "Um salvamento mais recente substituiu esta solicitação. Adicione ao chat após salvar.", "Запрос заменён более новым сохранением. Добавьте в чат после его завершения.", "استبدل حفظ أحدث هذا الطلب. أضف إلى الدردشة بعد اكتمال الحفظ.", "नए सेव ने इस अनुरोध को बदल दिया। सहेजने के बाद चैट में फिर जोड़ें।"],
    ChatNoWorkspace => ["No workspace folder is available for the terminal.", "终端没有可用的工作区目录。", "終端機沒有可用的工作區目錄。", "ターミナル用のワークスペースフォルダーがありません。", "터미널에 사용할 작업 공간 폴더가 없습니다.", "Aucun dossier d'espace de travail disponible pour le terminal.", "Kein Arbeitsbereichsordner für das Terminal verfügbar.", "No hay carpeta de trabajo disponible para el terminal.", "Nenhuma pasta de trabalho disponível para o terminal.", "Нет доступной папки рабочей области для терминала.", "لا يتوفر مجلد مساحة عمل للطرفية.", "टर्मिनल के लिए कार्यक्षेत्र फ़ोल्डर उपलब्ध नहीं है।"],
    CopyImageFailed => ["Could not copy image", "无法复制图片", "無法複製圖片", "画像をコピーできません", "이미지를 복사할 수 없음", "Impossible de copier l'image", "Bild konnte nicht kopiert werden", "No se pudo copiar la imagen", "Não foi possível copiar a imagem", "Не удалось скопировать изображение", "تعذر نسخ الصورة", "छवि कॉपी नहीं हो सकी"],
    UnknownCommand => ["Unknown command", "未知命令", "未知指令", "不明なコマンド", "알 수 없는 명령", "Commande inconnue", "Unbekannter Befehl", "Comando desconocido", "Comando desconhecido", "Неизвестная команда", "أمر غير معروف", "अज्ञात कमांड"],
    ProfileStarted => ["Runtime profile started", "已开始运行时性能分析", "已開始執行時效能分析", "実行時プロファイルを開始しました", "런타임 프로파일링 시작됨", "Profilage d'exécution démarré", "Laufzeitprofilierung gestartet", "Perfil de ejecución iniciado", "Perfil de execução iniciado", "Профилирование запущено", "بدأ تحليل أداء التشغيل", "रनटाइम प्रोफ़ाइल शुरू हुई"],
    ProfileStopped => ["Runtime profile stopped", "已停止运行时性能分析", "已停止執行時效能分析", "実行時プロファイルを停止しました", "런타임 프로파일링 중지됨", "Profilage d'exécution arrêté", "Laufzeitprofilierung beendet", "Perfil de ejecución detenido", "Perfil de execução parado", "Профилирование остановлено", "توقف تحليل أداء التشغيل", "रनटाइम प्रोफ़ाइल बंद हुई"],
    ProfileStartFailed => ["Failed to start runtime profile", "无法开始运行时性能分析", "無法開始執行時效能分析", "実行時プロファイルを開始できません", "런타임 프로파일링을 시작할 수 없음", "Échec du démarrage du profilage", "Laufzeitprofilierung konnte nicht gestartet werden", "No se pudo iniciar el perfil de ejecución", "Falha ao iniciar perfil de execução", "Не удалось запустить профилирование", "تعذر بدء تحليل الأداء", "रनटाइम प्रोफ़ाइल शुरू नहीं हो सकी"],
    ProfileStopFailed => ["Failed to stop runtime profile", "无法停止运行时性能分析", "無法停止執行時效能分析", "実行時プロファイルを停止できません", "런타임 프로파일링을 중지할 수 없음", "Échec de l'arrêt du profilage", "Laufzeitprofilierung konnte nicht beendet werden", "No se pudo detener el perfil de ejecución", "Falha ao parar perfil de execução", "Не удалось остановить профилирование", "تعذر إيقاف تحليل الأداء", "रनटाइम प्रोफ़ाइल बंद नहीं हो सकी"],
    ProfileNotRunning => ["Runtime profile is not running", "尚未开始运行时性能分析", "尚未開始執行時效能分析", "実行時プロファイルは動作していません", "런타임 프로파일링이 실행 중이 아님", "Le profilage n'est pas en cours", "Laufzeitprofilierung läuft nicht", "El perfil de ejecución no está activo", "O perfil de execução não está ativo", "Профилирование не запущено", "تحليل الأداء غير قيد التشغيل", "रनटाइम प्रोफ़ाइल नहीं चल रही"],
    ProfileSnapshotWritten => ["Diagnostic snapshot saved", "已保存诊断快照", "已儲存診斷快照", "診断スナップショットを保存しました", "진단 스냅샷 저장됨", "Instantané de diagnostic enregistré", "Diagnoseschnappschuss gespeichert", "Instantánea de diagnóstico guardada", "Instantâneo de diagnóstico salvo", "Диагностический снимок сохранён", "حُفظت اللقطة التشخيصية", "निदान स्नैपशॉट सहेजा गया"],
    ProfileSnapshotFailed => ["Failed to save diagnostic snapshot", "无法保存诊断快照", "無法儲存診斷快照", "診断スナップショットを保存できません", "진단 스냅샷을 저장할 수 없음", "Échec de l'enregistrement du diagnostic", "Diagnoseschnappschuss konnte nicht gespeichert werden", "No se pudo guardar la instantánea", "Falha ao salvar instantâneo", "Не удалось сохранить диагностический снимок", "تعذر حفظ اللقطة التشخيصية", "निदान स्नैपशॉट सहेजा नहीं जा सका"],
    ProfileOutput => ["Profile output directory", "性能分析输出目录", "效能分析輸出目錄", "プロファイル出力先", "프로파일 출력 폴더", "Dossier de sortie du profilage", "Profil-Ausgabeordner", "Carpeta de salida del perfil", "Pasta de saída do perfil", "Папка результатов профилирования", "مجلد نتائج التحليل", "प्रोफ़ाइल आउटपुट फ़ोल्डर"],
    ProfileInstructions => ["Press {stop} to stop, or {snapshot} to save a snapshot.", "按 {stop} 停止分析，或按 {snapshot} 保存快照。", "按 {stop} 停止分析，或按 {snapshot} 儲存快照。", "{stop} で停止、{snapshot} でスナップショットを保存します。", "{stop}로 중지하거나 {snapshot}로 스냅샷을 저장하세요.", "Appuyez sur {stop} pour arrêter, ou {snapshot} pour enregistrer un instantané.", "Mit {stop} stoppen oder mit {snapshot} einen Schnappschuss speichern.", "Pulse {stop} para detener o {snapshot} para guardar una instantánea.", "Pressione {stop} para parar ou {snapshot} para salvar um instantâneo.", "Нажмите {stop} для остановки или {snapshot} для сохранения снимка.", "اضغط {stop} للإيقاف أو {snapshot} لحفظ لقطة.", "रोकने के लिए {stop} या स्नैपशॉट सहेजने के लिए {snapshot} दबाएँ।"],
    ProfileStartInstruction => ["Press {shortcut} to start profiling before taking a snapshot.", "请先按 {shortcut} 开始分析，再保存快照。", "請先按 {shortcut} 開始分析，再儲存快照。", "スナップショットを作成する前に {shortcut} で開始してください。", "스냅샷을 만들기 전에 {shortcut}로 프로파일링을 시작하세요.", "Appuyez sur {shortcut} pour lancer le profilage avant de créer un instantané.", "Vor dem Schnappschuss die Profilierung mit {shortcut} starten.", "Pulse {shortcut} para iniciar el perfil antes de tomar una instantánea.", "Pressione {shortcut} para iniciar o perfil antes de criar um instantâneo.", "Перед созданием снимка запустите профилирование нажатием {shortcut}.", "اضغط {shortcut} لبدء التحليل قبل أخذ لقطة.", "स्नैपशॉट लेने से पहले {shortcut} से प्रोफ़ाइल शुरू करें।"],
    ProfileTimeline => ["JSON timeline", "JSON 时间线", "JSON 時間軸", "JSON タイムライン", "JSON 타임라인", "Chronologie JSON", "JSON-Zeitleiste", "Cronología JSON", "Linha do tempo JSON", "Временная шкала JSON", "الخط الزمني JSON", "JSON समयरेखा"],
    ProfileStacks => ["CPU folded stacks", "CPU 折叠调用栈", "CPU 摺疊呼叫堆疊", "CPU 折りたたみスタック", "CPU 접힌 스택", "Piles CPU repliées", "Gefaltete CPU-Stacks", "Pilas CPU plegadas", "Pilhas CPU recolhidas", "Свёрнутые стеки CPU", "مكدسات CPU المطوية", "CPU समेटे गए स्टैक"],
    TableColumn => ["Column", "列", "欄", "列", "열", "Colonne", "Spalte", "Columna", "Coluna", "Столбец", "عمود", "स्तंभ"],
    TableCell => ["Cell", "单元格", "儲存格", "セル", "셀", "Cellule", "Zelle", "Celda", "Célula", "Ячейка", "خلية", "सेल"]
}

impl MessageKey {
    pub(crate) fn format(self, language: UiLanguage, arguments: &[(&str, &str)]) -> String {
        let mut template = self.text(language);
        let mut result = String::new();
        while let Some((before, after)) = template.split_once('{') {
            result.push_str(before);
            let (key, rest) = after.split_once('}').expect("valid localized placeholder");
            let value = arguments
                .iter()
                .find(|(name, _)| *name == key)
                .expect("localized message argument is present")
                .1;
            result.push_str(value);
            template = rest;
        }
        result.push_str(template);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(text: &str) -> Vec<&str> {
        let mut keys = text
            .split('{')
            .skip(1)
            .map(|part| part.split_once('}').unwrap().0)
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }

    #[test]
    fn ui_messages_cover_all_languages_with_identical_placeholders() {
        for key in MessageKey::ALL {
            let english = key.text(UiLanguage::English);
            for &language in UiLanguage::supported() {
                let text = key.text(language);
                assert!(!text.is_empty(), "{key:?} {language:?}");
                assert_eq!(
                    placeholders(text),
                    placeholders(english),
                    "{key:?} {language:?}"
                );
                if language != UiLanguage::English {
                    assert_ne!(text, english, "{key:?} untranslated for {language:?}");
                }
            }
        }
        let anchor = "{snapshot}<x>";
        assert!(
            MessageKey::AnchorMissing
                .format(UiLanguage::ChineseSimplified, &[("anchor", anchor)])
                .ends_with(anchor)
        );
    }
}
