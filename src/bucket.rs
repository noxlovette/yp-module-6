/// Сведение о предмете в некотором количестве
#[derive(Debug, Clone, PartialEq)]
pub struct Bucket {
    asset_id: String,
    count: u32,
}

impl Bucket {
    pub fn new(asset_id: impl Into<String>, count: u32) -> Self {
        Self {
            asset_id: asset_id.into(),
            count,
        }
    }
}
