use crate::error::ParsingError;
use std::{num::NonZeroU32, ops::Deref, str::FromStr};

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
impl Bucket {
    pub fn new(asset_id: AssetIdentifier, count: u32) -> Self {
        Self {
            asset_id: asset_id.into(),
            count,
        }
    }
}

/// [Bucket] конкретного пользователя
#[derive(Debug, Clone, PartialEq)]
pub struct UserBucket {
    user_id: UserId,
    bucket: Bucket,
}

impl UserBucket {
    pub fn new(uid: UserId, b: Bucket) -> Self {
        Self {
            user_id: uid,
            bucket: b,
        }
    }
}

impl UserBuckets {
    pub fn new(uid: UserId, bs: Vec<Bucket>) -> Self {
        Self {
            user_id: uid,
            buckets: bs,
        }
    }
}

/// [Бакеты](Bucket) конкретного пользователя
#[derive(Debug, Clone, PartialEq)]
pub struct UserBuckets {
    user_id: UserId,
    buckets: Vec<Bucket>,
}

/// Фиатные деньги конкретного пользователя
#[derive(Debug, Clone, PartialEq)]
pub struct UserCash {
    user_id: UserId,
    count: Capital,
}

/// Капитал, не меньше 10usd
#[derive(Debug, Clone, PartialEq)]
pub struct Capital(NonZeroU32);

impl Capital {
    pub const MIN: u32 = 10;

    pub fn new(count: NonZeroU32) -> Result<Self, ParsingError> {
        if count.get() < Self::MIN {
            Err(ParsingError::CapitalTooLow)
        } else {
            Ok(Self(count))
        }
    }

    pub fn get(&self) -> u32 {
        self.0.get()
    }
}

impl UserCash {
    pub fn new(user_id: UserId, count: Capital) -> Self {
        Self { user_id, count }
    }
}

/// Ликвидность актива, не меньше 50usd
#[derive(Debug, Clone, PartialEq)]
pub struct Liquidity(NonZeroU32);

impl Liquidity {
    pub const MIN: u32 = 50;

    pub fn new(count: NonZeroU32) -> Result<Self, ParsingError> {
        if count.get() < Self::MIN {
            Err(ParsingError::LiquidityTooLow)
        } else {
            Ok(Self(count))
        }
    }

    pub fn get(&self) -> u32 {
        self.0.get()
    }
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
