use crate::error::ParsingError;
use std::{ops::Deref, str::FromStr};

#[derive(Debug, Clone, PartialEq)]
pub struct AssetIdentifier(String);

impl FromStr for AssetIdentifier {
    type Err = ParsingError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            Err(ParsingError::ParseUserIdError)
        } else {
            Ok(Self(s.into()))
        }
    }
}

impl AsRef<str> for AssetIdentifier {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Deref for AssetIdentifier {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserId(String);

impl FromStr for UserId {
    type Err = ParsingError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            Err(ParsingError::ParseUserIdError)
        } else {
            Ok(Self(s.into()))
        }
    }
}

impl AsRef<str> for UserId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
impl Deref for UserId {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Сведение о предмете в некотором количестве
#[derive(Debug, Clone, PartialEq)]
pub struct Bucket {
    asset_id: AssetIdentifier,
    count: u32,
}

/// [Bucket] конкретного пользователя
#[derive(Debug, Clone, PartialEq)]
pub struct UserBucket {
    pub user_id: UserId,
    pub bucket: Bucket,
}

/// [Бакеты](Bucket) конкретного пользователя
#[derive(Debug, Clone, PartialEq)]
pub struct UserBuckets {
    pub user_id: String,
    pub buckets: Vec<Bucket>,
}
/// Фиатные деньги конкретного пользователя
#[derive(Debug, Clone, PartialEq)]
pub struct UserCash {
    pub user_id: String,
    pub count: u32,
}

pub const AUTHDATA_SIZE: usize = 1024;
// подсказка: довольно много места на стэке
/// Данные для авторизации
#[derive(Debug, Clone, PartialEq)]
pub struct AuthData([u8; AUTHDATA_SIZE]);

impl AuthData {
    pub fn new(d: [u8; AUTHDATA_SIZE]) -> Self {
        Self(d)
    }
}

impl Bucket {
    pub fn new(asset_id: AssetIdentifier, count: u32) -> Self {
        Self {
            asset_id: asset_id.into(),
            count,
        }
    }
}
/// Список опубликованных бакетов
#[derive(Debug, Clone, PartialEq)]
pub struct Announcements(Vec<UserBuckets>);

impl Announcements {
    pub fn new(v: Vec<UserBuckets>) -> Self {
        Self(v)
    }
}

impl From<Vec<UserBuckets>> for Announcements {
    fn from(value: Vec<UserBuckets>) -> Self {
        Self(value)
    }
}
