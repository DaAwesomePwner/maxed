use maxed_types::AlertKind;
use serde::{de::Visitor, Deserialize, Deserializer};

/// Ask the derived Serde deserializer for its actual variant tags so adding a
/// variant without extending ALL fails without a second manually maintained list.
#[test]
fn all_covers_every_serde_tag() {
    struct Tags;
    impl<'de> Deserializer<'de> for Tags {
        type Error = serde::de::value::Error;
        fn deserialize_enum<V: Visitor<'de>>(
            self,
            _name: &'static str,
            variants: &'static [&'static str],
            _visitor: V,
        ) -> Result<V::Value, Self::Error> {
            let actual: Vec<String> = AlertKind::ALL
                .iter()
                .map(|kind| {
                    serde_json::to_value(kind)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_owned()
                })
                .collect();
            assert_eq!(actual, variants);
            Err(serde::de::Error::custom("tags checked"))
        }
        fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Self::Error> {
            panic!("AlertKind must deserialize as an enum")
        }
        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf
            option unit unit_struct newtype_struct seq tuple tuple_struct map struct
            identifier ignored_any
        }
    }
    assert!(AlertKind::deserialize(Tags).is_err());
}

#[test]
fn key_pop_defaults_are_quiet_and_saved_choices_survive() {
    use maxed_types::{AlertLoudness, AlertPreferences};
    let defaults = AlertPreferences::default();
    assert_eq!(defaults.key_popped, AlertLoudness::Off);
    assert_eq!(defaults.key_pop_dungeons, Some(vec![]));
    let mut legacy = serde_json::to_value(&defaults).unwrap();
    legacy.as_object_mut().unwrap().remove("key_popped");
    legacy.as_object_mut().unwrap().remove("key_pop_dungeons");
    let unset: AlertPreferences = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(unset.key_popped, AlertLoudness::Off);
    // Legacy explicit loudness without a filter means all dungeons, as before.
    legacy["key_popped"] = serde_json::json!("toast");
    let saved: AlertPreferences = serde_json::from_value(legacy).unwrap();
    assert_eq!(saved.key_popped, AlertLoudness::Toast);
    assert!(saved.includes_key_pop_dungeon("Synthetic dungeon"));
    for selection in [None, Some(vec![]), Some(vec!["Synthetic dungeon".into()])] {
        let saved = AlertPreferences {
            key_popped: AlertLoudness::Banner,
            key_pop_dungeons: selection,
            key_pop_excluded_dungeons: vec!["Synthetic excluded dungeon".into()],
            ..defaults.clone()
        };
        assert_eq!(
            serde_json::from_value::<AlertPreferences>(serde_json::to_value(&saved).unwrap())
                .unwrap(),
            saved
        );
    }
}
