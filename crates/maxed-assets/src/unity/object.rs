#[derive(Debug)]
pub(crate) struct Object {
    pub path_id: i64,
    pub offset: u64,
    pub size: u32,
    pub class: i32,
}
