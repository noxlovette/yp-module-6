use std::num::NonZeroU32;

use crate::domain::{
    Announcements, AssetIdentifier, AuthData, Liquidity, UserBucket, UserCash,
    UserId,
};

/// Строка логов, [лог](AppLogKind) с `request_id`
#[derive(Debug, Clone, PartialEq)]
pub struct LogLine {
    kind: LogKind,
    request_id: u32,
}

/// Все виды логов
#[derive(Debug, Clone, PartialEq)]
pub enum LogKind {
    System(SystemLogKind),
    App(AppLogKind),
}

impl LogLine {
    pub fn new(kind: LogKind, request_id: u32) -> Self {
        Self { kind, request_id }
    }

    pub fn request_id(&self) -> u32 {
        self.request_id
    }

    pub fn is_error(&self) -> bool {
        self.kind.is_error()
    }

    pub fn is_exchange(&self) -> bool {
        self.kind.is_exchange()
    }
}

impl LogKind {
    pub fn is_error(&self) -> bool {
        matches!(
            &self,
            LogKind::System(SystemLogKind::Error(_))
                | LogKind::App(AppLogKind::Error(_))
        )
    }

    pub fn is_exchange(&self) -> bool {
        matches!(
            &self,
            LogKind::App(AppLogKind::Journal(
                AppLogJournalKind::BuyAsset(_)
                    | AppLogJournalKind::SellAsset(_)
                    | AppLogJournalKind::CreateUser { .. }
                    | AppLogJournalKind::RegisterAsset { .. }
                    | AppLogJournalKind::DepositCash(_)
                    | AppLogJournalKind::WithdrawCash(_)
            ))
        )
    }
}

/// Все виды [системных](LogKind) логов
#[derive(Debug, Clone, PartialEq)]
pub enum SystemLogKind {
    Error(SystemLogErrorKind),
    Trace(SystemLogTraceKind),
}
/// Trace [системы](SystemLogKind)
#[derive(Debug, Clone, PartialEq)]
pub enum SystemLogTraceKind {
    SendRequest(String),
    GetResponse(String),
}
/// Error [системы](SystemLogKind)
#[derive(Debug, Clone, PartialEq)]
pub enum SystemLogErrorKind {
    NetworkError(String),
    AccessDenied(String),
}
/// Все виды [логов приложения](LogKind) логов
#[derive(Debug, Clone, PartialEq)]
pub enum AppLogKind {
    Error(AppLogErrorKind),
    Trace(AppLogTraceKind),
    Journal(AppLogJournalKind),
}
/// Error [приложения](AppLogKind)
#[derive(Debug, Clone, PartialEq)]
pub enum AppLogErrorKind {
    LackOf(String),
    SystemError(String),
}
/// Trace [приложения](AppLogKind)
#[derive(Debug, Clone, PartialEq)]
pub enum AppLogTraceKind {
    Connect(Box<AuthData>),
    SendRequest(String),
    Check(Announcements),
    GetResponse(String),
}
/// Журнал [приложения](AppLogKind), самые высокоуровневые события
#[derive(Debug, Clone, PartialEq)]
pub enum AppLogJournalKind {
    CreateUser {
        user_id: UserId,
        authorized_capital: NonZeroU32,
    },
    DeleteUser {
        user_id: UserId,
    },
    RegisterAsset {
        asset_id: AssetIdentifier,
        user_id: UserId,
        liquidity: Liquidity,
    },
    UnregisterAsset {
        asset_id: AssetIdentifier,
        user_id: UserId,
    },
    DepositCash(UserCash),
    WithdrawCash(UserCash),
    BuyAsset(UserBucket),
    SellAsset(UserBucket),
}
