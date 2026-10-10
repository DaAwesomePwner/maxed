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
