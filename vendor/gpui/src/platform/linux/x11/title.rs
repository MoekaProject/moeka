#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TitleProperty {
    NetWmName,
    WmName,
}

pub(super) fn write_title_properties<E>(
    title: &str,
    mut write: impl FnMut(TitleProperty, &[u8]) -> Result<(), E>,
) -> Result<(), E> {
    let net = write(TitleProperty::NetWmName, title.as_bytes());
    let legacy = write(TitleProperty::WmName, &legacy_wm_name(title));
    net.and(legacy)
}

fn legacy_wm_name(title: &str) -> Vec<u8> {
    title
        .chars()
        .map(|character| match character {
            '\0' => b'?',
            character if (character as u32) <= 0xff => character as u8,
            _ => b'?',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{TitleProperty, legacy_wm_name, write_title_properties};

    #[test]
    fn legacy_title_is_latin1_not_utf8() {
        assert_eq!(legacy_wm_name("Café"), b"Caf\xe9");
        assert_eq!(legacy_wm_name("小语文稿 😀"), b"???? ?");
        assert_eq!(legacy_wm_name("A\0B"), b"A?B");
        assert_eq!(legacy_wm_name(""), b"");
    }

    #[test]
    fn both_title_properties_are_written_with_their_expected_bytes() {
        let mut writes = Vec::new();
        write_title_properties("Café 小语文稿", |property, bytes| {
            writes.push((property, bytes.to_vec()));
            Ok::<_, ()>(())
        })
        .unwrap();
        assert_eq!(
            writes,
            vec![
                (
                    TitleProperty::NetWmName,
                    "Café 小语文稿".as_bytes().to_vec()
                ),
                (TitleProperty::WmName, b"Caf\xe9 ????".to_vec()),
            ]
        );
    }

    #[test]
    fn either_failure_is_reported_without_skipping_the_other_property() {
        for failing in [TitleProperty::NetWmName, TitleProperty::WmName] {
            let mut attempted = Vec::new();
            let result = write_title_properties("title", |property, _| {
                attempted.push(property);
                if property == failing {
                    Err(property)
                } else {
                    Ok(())
                }
            });
            assert_eq!(result, Err(failing));
            assert_eq!(attempted, [TitleProperty::NetWmName, TitleProperty::WmName]);
        }
    }
}
