use std::{borrow::Cow, num::NonZeroU32};

use crate::{
    domain::{
        AUTHDATA_SIZE, Announcements, AssetIdentifier, AuthData, Bucket,
        UserBucket, UserBuckets, UserCash, UserId,
    },
    error::ParsingError,
    log::{
        AppLogErrorKind, AppLogJournalKind, AppLogKind, AppLogTraceKind,
        LogKind, LogLine, SystemLogErrorKind, SystemLogKind,
        SystemLogTraceKind,
    },
};

/// Трейт, чтобы **реализовывать** и **требовать** метод 'распарсь и покажи,
/// что распарсить осталось'
trait Parser<'a> {
    type Dest;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError>;
}
/// Вспомогательный трейт, чтобы писать собственный десериализатор
/// (по решаемой задаче - отдалённый аналог `serde::Deserialize`)
pub trait Parsable: Sized {
    type Parser: for<'a> Parser<'a, Dest = Self>;
    fn parser() -> Self::Parser;
}

mod stdp {
    use std::num::{NonZeroI32, NonZeroU32};

    use crate::error::ParsingError;

    // parsers for std types
    use super::Parser;

    // ноль - невалидное значение: в наших логах нулей нет, поэтому парсим
    // сразу в NonZero* - `self` не используется, значение всегда одно и то же
    impl<'a> Parser<'a> for NonZeroU32 {
        type Dest = NonZeroU32;

        fn parse(
            &self,
            input: &'a str,
        ) -> Result<(&'a str, Self::Dest), ParsingError> {
            let (remaining, is_hex) = input
                .strip_prefix("0x")
                .map_or((input, false), |remaining| (remaining, true));

            let end_idx = remaining
                .char_indices()
                .find_map(|(idx, c)| match (is_hex, c) {
                    (true, 'a'..='f' | '0'..='9' | 'A'..='F') => None,
                    (false, '0'..='9') => None,
                    _ => Some(idx),
                })
                .unwrap_or(remaining.len());
            let value = u32::from_str_radix(
                &remaining[..end_idx],
                if is_hex { 16 } else { 10 },
            )?;

            Ok((
                &remaining[end_idx..],
                NonZeroU32::new(value)
                    .ok_or(ParsingError::ParseNonZeroIntError)?,
            ))
        }
    }
    impl<'a> Parser<'a> for NonZeroI32 {
        type Dest = NonZeroI32;

        fn parse(
            &self,
            input: &'a str,
        ) -> Result<(&'a str, Self::Dest), ParsingError> {
            let end_idx = input
                .char_indices()
                .skip(1)
                .find_map(|(idx, c)| (!c.is_ascii_digit()).then_some(idx))
                .unwrap_or(input.len());

            let value = input[..end_idx].parse()?;

            Ok((
                &input[end_idx..],
                NonZeroI32::new(value)
                    .ok_or(ParsingError::ParseNonZeroIntError)?,
            ))
        }
    }

    /// Шестнадцатеричные байты (пригодится при парсинге блобов)
    #[derive(Debug, Clone)]
    pub struct Byte;
    impl<'a> Parser<'a> for Byte {
        type Dest = u8;

        fn parse(
            &self,
            input: &'a str,
        ) -> Result<(&'a str, Self::Dest), ParsingError> {
            let (to_parse, remaining) = input
                .split_at_checked(2)
                .ok_or(ParsingError::SplitStringError)?;
            // the check was unnecessary cause from_str_radix will already
            // validate hex digits
            let value = u8::from_str_radix(to_parse, 16)?;
            Ok((&remaining, value))
        }
    }
}

/// Обернуть строку в кавычки, экранировав кавычки, которые в строке уже есть
fn quote(input: &str) -> String {
    let mut result = String::from("\"");
    result.extend(
        input
            .chars()
            .map(|c| match c {
                '\\' | '"' => ['\\', c].into_iter().take(2),
                _ => [c, ' '].into_iter().take(1),
            })
            .flatten(),
    );
    result.push('"');
    result
}
/// Распарсить строку, которую ранее [обернули в кавычки](quote)
///
/// `"abc\"def\\ghi"nice` -> (`abcd"def\ghi`, `nice`)
fn do_unquote(input: &str) -> Result<(&str, Cow<str>), ParsingError> {
    let body = input
        .strip_prefix('"')
        .ok_or(ParsingError::ParseQuotedString)?;
    // найти первый спецсимвол: если это закрывающая кавычка раньше любого
    // экранирования - можно просто вырезать подстроку без посимвольной сборки
    match body.find(|c| c == '"' || c == '\\') {
        Some(idx) if body.as_bytes()[idx] == b'"' => {
            Ok((&body[idx + 1..], Cow::Borrowed(&body[..idx])))
        }
        Some(idx) => {
            // до первого '\\' экранирования нет - копируем префикс одним
            // куском, а не по символу, и достраиваем результат уже
            // с учётом экранирования
            let mut result = String::from(&body[..idx]);
            let mut escaped_now = false;
            let mut chars = body[idx..].chars();
            while let Some(c) = chars.next() {
                match (c, escaped_now) {
                    ('"' | '\\', true) => {
                        result.push(c);
                        escaped_now = false;
                    }
                    ('\\', false) => escaped_now = true,
                    ('"', false) => {
                        return Ok((chars.as_str(), Cow::Owned(result)));
                    }
                    (c, _) => {
                        result.push(c);
                        escaped_now = false;
                    }
                }
            }
            Err(ParsingError::ParseQuotedString)
        }
        None => Err(ParsingError::ParseQuotedString),
    }
}

/// Парсер кавычек
#[derive(Debug, Clone)]
pub struct Unquote;
impl<'a> Parser<'a> for Unquote {
    type Dest = Cow<'a, str>;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        do_unquote(input)
    }
}
/// Конструктор [Unquote]
fn unquote() -> Unquote {
    Unquote
}
/// Парсер id пользователя (провалидированного через
/// [FromStr](std::str::FromStr))
#[derive(Debug, Clone)]
pub struct ParseUserId;
impl<'a> Parser<'a> for ParseUserId {
    type Dest = UserId;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        let (remaining, s) = do_unquote(input)?;
        Ok((remaining, s.parse()?))
    }
}
/// Конструктор [ParseUserId]
fn user_id() -> ParseUserId {
    ParseUserId
}
/// Парсер id предмета (провалидированного через [FromStr](std::str::FromStr))
#[derive(Debug, Clone)]
pub struct ParseAssetIdentifier;
impl<'a> Parser<'a> for ParseAssetIdentifier {
    type Dest = AssetIdentifier;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        let (remaining, s) = do_unquote(input)?;
        Ok((remaining, s.parse()?))
    }
}
/// Конструктор [ParseAssetIdentifier]
fn asset_identifier() -> ParseAssetIdentifier {
    ParseAssetIdentifier
}
/// Парсер, возвращающий результат как есть
#[derive(Debug, Clone)]
struct AsIs;
impl<'a> Parser<'a> for AsIs {
    type Dest = &'a str;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        Ok((&input[input.len()..], input))
    }
}
/// Парсер константных строк
/// (аналог `nom::bytes::complete::tag`)
#[derive(Debug, Clone)]
pub struct Tag(&'static str);

impl Tag {
    pub fn new(t: &'static str) -> Self {
        Self(t)
    }
}
impl<'a> Parser<'a> for Tag {
    type Dest = ();

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        Ok((
            input
                .strip_prefix(self.0)
                .ok_or(ParsingError::ParseTagError)?,
            (),
        ))
    }
}
/// Конструктор [Tag]
fn tag(t: &'static str) -> Tag {
    Tag::new(t)
}

/// Распарсить строку, обёрнутую в кавычки
/// (сокращённая версия [do_unquote], в которой вложенные кавычки не
/// предусмотрены)
fn do_unquote_non_escaped(input: &str) -> Result<(&str, &str), ParsingError> {
    let body = input
        .strip_prefix('"')
        .ok_or(ParsingError::ParseQuotedString)?;
    let quote_idx = body.find('"').ok_or(ParsingError::ParseQuotedString)?;
    if quote_idx == 0 || body.as_bytes().get(quote_idx - 1) == Some(&b'\\') {
        return Err(ParsingError::ParseQuotedString);
    }
    Ok((&body[quote_idx + 1..], &body[..quote_idx]))
}

/// Парсер [тэга](Tag), обёрнутого в кавычки
#[derive(Debug, Clone)]
struct QuotedTag(Tag);

impl<'a> Parser<'a> for QuotedTag {
    type Dest = ();

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        let (remaining, candidate) = do_unquote_non_escaped(input)?;
        if !self.0.parse(candidate)?.0.is_empty() {
            return Err(ParsingError::ParseTagError);
        }
        Ok((remaining, ()))
    }
}
/// Конструктор [QuotedTag]
fn quoted_tag(t: &'static str) -> QuotedTag {
    QuotedTag(Tag::new(t))
}
/// Комбинатор, пробрасывающий строку без лидирующих пробелов
#[derive(Debug, Clone)]
pub struct StripWhitespace<T> {
    parser: T,
}
impl<'a, T: Parser<'a>> Parser<'a> for StripWhitespace<T> {
    type Dest = T::Dest;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        self.parser
            .parse(input.trim_start())
            .map(|(remaining, parsed)| (remaining.trim_start(), parsed))
    }
}
/// Конструктор [StripWhitespace]
fn strip_whitespace<T: for<'a> Parser<'a>>(parser: T) -> StripWhitespace<T> {
    StripWhitespace { parser }
}
/// Комбинатор, чтобы распарсить нужное, окружённое в начале и в конце чем-то
/// обязательным, не участвующем в результате.
/// Пробрасывает строку в парсер1, оставшуюся строку после первого
/// парсинга - в парсер2, оставшуюся строку после второго парсинга - в парсер3.
/// Результат парсера2 будет результатом этого комбинатора, а оставшейся
/// строкой - строка, оставшаяся после парсера3.
/// (аналог `delimited` из `nom`)
#[derive(Debug, Clone)]
pub struct Delimited<Prefix, T, Suffix> {
    prefix_to_ignore: Prefix,
    dest_parser: T,
    suffix_to_ignore: Suffix,
}
impl<'a, Prefix, T, Suffix> Parser<'a> for Delimited<Prefix, T, Suffix>
where
    Prefix: Parser<'a>,
    T: Parser<'a>,
    Suffix: Parser<'a>,
{
    type Dest = T::Dest;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        let (remaining, _) = self.prefix_to_ignore.parse(input)?;
        let (remaining, result) = self.dest_parser.parse(remaining)?;
        self.suffix_to_ignore
            .parse(remaining)
            .map(|(remaining, _)| (remaining, result))
    }
}
/// Конструктор [Delimited]
fn delimited<Prefix, T, Suffix>(
    prefix_to_ignore: Prefix,
    dest_parser: T,
    suffix_to_ignore: Suffix,
) -> Delimited<Prefix, T, Suffix>
where
    Prefix: for<'a> Parser<'a>,
    T: for<'a> Parser<'a>,
    Suffix: for<'a> Parser<'a>,
{
    Delimited {
        prefix_to_ignore,
        dest_parser,
        suffix_to_ignore,
    }
}
/// Комбинатор-отображение. Парсит дочерним парсером, преобразует результат так,
/// как вызывающему хочется
#[derive(Debug, Clone)]
pub struct Map<T, M> {
    parser: T,
    map: M,
}
impl<'a, T: Parser<'a>, Dest: Sized, M: Fn(T::Dest) -> Dest> Parser<'a>
    for Map<T, M>
{
    type Dest = Dest;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        self.parser
            .parse(input)
            .map(|(remaining, pre_result)| (remaining, (self.map)(pre_result)))
    }
}
/// Конструктор [Map]
fn map<T, Dest, M>(parser: T, map: M) -> Map<T, M>
where
    T: for<'a> Parser<'a>,
    M: for<'a> Fn(<T as Parser<'a>>::Dest) -> Dest,
{
    Map { parser, map }
}
/// Комбинатор с отбрасываемым префиксом, упрощённая версия [Delimited]
/// (аналог `preceeded` из `nom`)
#[derive(Debug, Clone)]
pub struct Preceded<Prefix, T> {
    prefix_to_ignore: Prefix,
    dest_parser: T,
}
impl<'a, Prefix, T> Parser<'a> for Preceded<Prefix, T>
where
    Prefix: Parser<'a>,
    T: Parser<'a>,
{
    type Dest = T::Dest;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        let (remaining, _) = self.prefix_to_ignore.parse(input)?;
        self.dest_parser.parse(remaining)
    }
}
/// Конструктор [Preceded]
fn preceded<Prefix, T>(
    prefix_to_ignore: Prefix,
    dest_parser: T,
) -> Preceded<Prefix, T>
where
    Prefix: for<'a> Parser<'a>,
    T: for<'a> Parser<'a>,
{
    Preceded {
        prefix_to_ignore,
        dest_parser,
    }
}
/// Комбинатор, который требует, чтобы все дочерние парсеры отработали,
/// (аналог `all` из `nom`)
#[derive(Debug, Clone)]
pub struct All<T> {
    parser: T,
}
impl<'a, A0, A1> Parser<'a> for All<(A0, A1)>
where
    A0: Parser<'a>,
    A1: Parser<'a>,
{
    type Dest = (A0::Dest, A1::Dest);

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        let (remaining, a0) = self.parser.0.parse(input)?;
        self.parser
            .1
            .parse(remaining)
            .map(|(remaining, a1)| (remaining, (a0, a1)))
    }
}
/// Конструктор [All] для двух парсеров
fn all2<A0: for<'a> Parser<'a>, A1: for<'a> Parser<'a>>(
    a0: A0,
    a1: A1,
) -> All<(A0, A1)> {
    All { parser: (a0, a1) }
}
impl<'a, A0, A1, A2> Parser<'a> for All<(A0, A1, A2)>
where
    A0: Parser<'a>,
    A1: Parser<'a>,
    A2: Parser<'a>,
{
    type Dest = (A0::Dest, A1::Dest, A2::Dest);

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        let (remaining, a0) = self.parser.0.parse(input)?;
        let (remaining, a1) = self.parser.1.parse(remaining)?;
        self.parser
            .2
            .parse(remaining)
            .map(|(remaining, a2)| (remaining, (a0, a1, a2)))
    }
}
/// Конструктор [All] для трёх парсеров
fn all3<
    A0: for<'a> Parser<'a>,
    A1: for<'a> Parser<'a>,
    A2: for<'a> Parser<'a>,
>(
    a0: A0,
    a1: A1,
    a2: A2,
) -> All<(A0, A1, A2)> {
    All {
        parser: (a0, a1, a2),
    }
}
impl<'a, A0, A1, A2, A3> Parser<'a> for All<(A0, A1, A2, A3)>
where
    A0: Parser<'a>,
    A1: Parser<'a>,
    A2: Parser<'a>,
    A3: Parser<'a>,
{
    type Dest = (A0::Dest, A1::Dest, A2::Dest, A3::Dest);

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        let (remaining, a0) = self.parser.0.parse(input)?;
        let (remaining, a1) = self.parser.1.parse(remaining)?;
        let (remaining, a2) = self.parser.2.parse(remaining)?;
        self.parser
            .3
            .parse(remaining)
            .map(|(remaining, a3)| (remaining, (a0, a1, a2, a3)))
    }
}
/// Конструктор [All] для четырёх парсеров
fn all4<
    A0: for<'a> Parser<'a>,
    A1: for<'a> Parser<'a>,
    A2: for<'a> Parser<'a>,
    A3: for<'a> Parser<'a>,
>(
    a0: A0,
    a1: A1,
    a2: A2,
    a3: A3,
) -> All<(A0, A1, A2, A3)> {
    All {
        parser: (a0, a1, a2, a3),
    }
}
/// Комбинатор, который вытаскивает значения из пары `"ключ":значение,`.
/// Для простоты реализации, запятая всегда нужна в конце пары ключ-значение,
/// простое '"ключ":значение' читаться не будет
#[derive(Debug, Clone)]
pub struct KeyValue<T> {
    parser: Delimited<
        All<(StripWhitespace<QuotedTag>, StripWhitespace<Tag>)>,
        StripWhitespace<T>,
        StripWhitespace<Tag>,
    >,
}
impl<'a, T> Parser<'a> for KeyValue<T>
where
    T: Parser<'a>,
{
    type Dest = T::Dest;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        self.parser.parse(input)
    }
}
/// Конструктор [KeyValue]
fn key_value<T: for<'a> Parser<'a>>(
    key: &'static str,
    value_parser: T,
) -> KeyValue<T> {
    KeyValue {
        parser: delimited(
            all2(
                strip_whitespace(quoted_tag(key)),
                strip_whitespace(tag(":")),
            ),
            strip_whitespace(value_parser),
            strip_whitespace(tag(",")),
        ),
    }
}
/// Комбинатор, который возвращает результаты дочерних парсеров, если их
/// удалось применить друг после друга в любом порядке. Результат возвращается в
/// том порядке, в каком `Permutation` был сконструирован
/// (аналог `permutation` из `nom`)
#[derive(Debug, Clone)]
pub struct Permutation<T> {
    parsers: T,
}
impl<'a, A0, A1> Parser<'a> for Permutation<(A0, A1)>
where
    A0: Parser<'a>,
    A1: Parser<'a>,
{
    type Dest = (A0::Dest, A1::Dest);

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        match self.parsers.0.parse(input) {
            Ok((remaining, a0)) => self
                .parsers
                .1
                .parse(remaining)
                .map(|(remaining, a1)| (remaining, (a0, a1))),
            Err(_) => {
                self.parsers.1.parse(input).and_then(|(remaining, a1)| {
                    self.parsers
                        .0
                        .parse(remaining)
                        .map(|(remaining, a0)| (remaining, (a0, a1)))
                })
            }
        }
    }
}
/// Конструктор [Permutation] для двух парсеров
fn permutation2<A0: for<'a> Parser<'a>, A1: for<'a> Parser<'a>>(
    a0: A0,
    a1: A1,
) -> Permutation<(A0, A1)> {
    Permutation { parsers: (a0, a1) }
}
impl<'a, A0, A1, A2> Parser<'a> for Permutation<(A0, A1, A2)>
where
    A0: Parser<'a>,
    A1: Parser<'a>,
    A2: Parser<'a>,
{
    type Dest = (A0::Dest, A1::Dest, A2::Dest);

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        match self.parsers.0.parse(input) {
            Ok((remaining, a0)) => match self.parsers.1.parse(remaining) {
                Ok((remaining, a1)) => self
                    .parsers
                    .2
                    .parse(remaining)
                    .map(|(remaining, a2)| (remaining, (a0, a1, a2))),
                Err(_) => self.parsers.2.parse(remaining).and_then(
                    |(remaining, a2)| {
                        self.parsers
                            .1
                            .parse(remaining)
                            .map(|(remaining, a1)| (remaining, (a0, a1, a2)))
                    },
                ),
            },
            Err(_) => match self.parsers.1.parse(input) {
                Ok((remaining, a1)) => match self.parsers.0.parse(remaining) {
                    Ok((remaining, a0)) => self
                        .parsers
                        .2
                        .parse(remaining)
                        .map(|(remaining, a2)| (remaining, (a0, a1, a2))),
                    Err(_) => self.parsers.2.parse(remaining).and_then(
                        |(remaining, a2)| {
                            self.parsers.0.parse(remaining).map(
                                |(remaining, a0)| (remaining, (a0, a1, a2)),
                            )
                        },
                    ),
                },
                Err(_) => {
                    self.parsers.2.parse(input).and_then(|(remaining, a2)| {
                        match self.parsers.0.parse(remaining) {
                            Ok((remaining, a0)) => {
                                self.parsers.1.parse(remaining).map(
                                    |(remaining, a1)| (remaining, (a0, a1, a2)),
                                )
                            }
                            Err(_) => self.parsers.1.parse(remaining).and_then(
                                |(remaining, a1)| {
                                    self.parsers.0.parse(remaining).map(
                                        |(remaining, a0)| {
                                            (remaining, (a0, a1, a2))
                                        },
                                    )
                                },
                            ),
                        }
                    })
                }
            },
        }
    }
}
/// Конструктор [Permutation] для трёх парсеров
fn permutation3<
    A0: for<'a> Parser<'a>,
    A1: for<'a> Parser<'a>,
    A2: for<'a> Parser<'a>,
>(
    a0: A0,
    a1: A1,
    a2: A2,
) -> Permutation<(A0, A1, A2)> {
    Permutation {
        parsers: (a0, a1, a2),
    }
}
/// Комбинатор списка из любого числа элементов, которые надо читать
/// вложенным парсером. Граница списка определяется квадратными (`[`&`]`)
/// скобками.
/// Для простоты реализации, после каждого элемента списка должна быть запятая
#[derive(Debug, Clone)]
pub struct List<T> {
    parser: T,
}
impl<'a, T: Parser<'a>> Parser<'a> for List<T> {
    type Dest = Vec<T::Dest>;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        let mut remaining = input
            .trim_start()
            .strip_prefix('[')
            .ok_or(ParsingError::ParseListError)?
            .trim_start();
        let mut result = Vec::new();
        while !remaining.is_empty() {
            match remaining.strip_prefix(']') {
                Some(remaining) => {
                    return Ok((remaining.trim_start(), result));
                }
                None => {
                    let (new_remaining, item) = self.parser.parse(remaining)?;
                    let new_remaining = new_remaining
                        .trim_start()
                        .strip_prefix(',')
                        .ok_or(ParsingError::ParseListError)?
                        .trim_start();
                    result.push(item);
                    remaining = new_remaining;
                }
            }
        }
        Err(ParsingError::ParseListError)
    }
}
/// Конструктор для [List]
fn list<T: for<'a> Parser<'a>>(parser: T) -> List<T> {
    List { parser }
}
/// Комбинатор, который вернёт тот результат, который будет успешно
/// получен первым из дочерних комбинаторов
/// (аналог `alt` из `nom`)
#[derive(Debug, Clone)]
pub struct Alt<T> {
    parser: T,
}
impl<'a, A0, A1, Dest> Parser<'a> for Alt<(A0, A1)>
where
    A0: Parser<'a, Dest = Dest>,
    A1: Parser<'a, Dest = Dest>,
{
    type Dest = Dest;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        if let Ok(ok) = self.parser.0.parse(input) {
            return Ok(ok);
        }
        self.parser.1.parse(input)
    }
}
/// Конструктор [Alt] для двух парсеров
fn alt2<Dest, A0, A1>(a0: A0, a1: A1) -> Alt<(A0, A1)>
where
    A0: for<'a> Parser<'a, Dest = Dest>,
    A1: for<'a> Parser<'a, Dest = Dest>,
{
    Alt { parser: (a0, a1) }
}
impl<'a, A0, A1, A2, Dest> Parser<'a> for Alt<(A0, A1, A2)>
where
    A0: Parser<'a, Dest = Dest>,
    A1: Parser<'a, Dest = Dest>,
    A2: Parser<'a, Dest = Dest>,
{
    type Dest = Dest;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        // match вместо тут не подойдёт - нужно лениво
        if let Ok(ok) = self.parser.0.parse(input) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.1.parse(input) {
            return Ok(ok);
        }
        self.parser.2.parse(input)
    }
}
/// Конструктор [Alt] для трёх парсеров

fn alt3<Dest, A0, A1, A2>(a0: A0, a1: A1, a2: A2) -> Alt<(A0, A1, A2)>
where
    A0: for<'a> Parser<'a, Dest = Dest>,
    A1: for<'a> Parser<'a, Dest = Dest>,
    A2: for<'a> Parser<'a, Dest = Dest>,
{
    Alt {
        parser: (a0, a1, a2),
    }
}
impl<'a, A0, A1, A2, A3, Dest> Parser<'a> for Alt<(A0, A1, A2, A3)>
where
    A0: Parser<'a, Dest = Dest>,
    A1: Parser<'a, Dest = Dest>,
    A2: Parser<'a, Dest = Dest>,
    A3: Parser<'a, Dest = Dest>,
{
    type Dest = Dest;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        if let Ok(ok) = self.parser.0.parse(input) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.1.parse(input) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.2.parse(input) {
            return Ok(ok);
        }
        self.parser.3.parse(input)
    }
}
/// Конструктор [Alt] для четырёх парсеров

fn alt4<Dest, A0, A1, A2, A3>(
    a0: A0,
    a1: A1,
    a2: A2,
    a3: A3,
) -> Alt<(A0, A1, A2, A3)>
where
    A0: for<'a> Parser<'a, Dest = Dest>,
    A1: for<'a> Parser<'a, Dest = Dest>,
    A2: for<'a> Parser<'a, Dest = Dest>,
    A3: for<'a> Parser<'a, Dest = Dest>,
{
    Alt {
        parser: (a0, a1, a2, a3),
    }
}
impl<'a, A0, A1, A2, A3, A4, A5, A6, A7, Dest> Parser<'a>
    for Alt<(A0, A1, A2, A3, A4, A5, A6, A7)>
where
    A0: Parser<'a, Dest = Dest>,
    A1: Parser<'a, Dest = Dest>,
    A2: Parser<'a, Dest = Dest>,
    A3: Parser<'a, Dest = Dest>,
    A4: Parser<'a, Dest = Dest>,
    A5: Parser<'a, Dest = Dest>,
    A6: Parser<'a, Dest = Dest>,
    A7: Parser<'a, Dest = Dest>,
{
    type Dest = Dest;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        if let Ok(ok) = self.parser.0.parse(input) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.1.parse(input) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.2.parse(input) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.3.parse(input) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.4.parse(input) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.5.parse(input) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.6.parse(input) {
            return Ok(ok);
        }
        self.parser.7.parse(input)
    }
}
/// Конструктор [Alt] для восьми парсеров

#[allow(clippy::too_many_arguments)]
fn alt8<Dest, A0, A1, A2, A3, A4, A5, A6, A7>(
    a0: A0,
    a1: A1,
    a2: A2,
    a3: A3,
    a4: A4,
    a5: A5,
    a6: A6,
    a7: A7,
) -> Alt<(A0, A1, A2, A3, A4, A5, A6, A7)>
where
    A0: for<'a> Parser<'a, Dest = Dest>,
    A1: for<'a> Parser<'a, Dest = Dest>,
    A2: for<'a> Parser<'a, Dest = Dest>,
    A3: for<'a> Parser<'a, Dest = Dest>,
    A4: for<'a> Parser<'a, Dest = Dest>,
    A5: for<'a> Parser<'a, Dest = Dest>,
    A6: for<'a> Parser<'a, Dest = Dest>,
    A7: for<'a> Parser<'a, Dest = Dest>,
{
    Alt {
        parser: (a0, a1, a2, a3, a4, a5, a6, a7),
    }
}

/// Комбинатор для применения дочернего парсера N раз
/// (аналог `take` из `nom`)
pub struct Take<T> {
    count: usize,
    parser: T,
}
impl<'a, T: Parser<'a>> Parser<'a> for Take<T> {
    type Dest = Vec<T::Dest>;

    fn parse(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, Self::Dest), ParsingError> {
        let mut remaining = input;
        let mut result = Vec::new();
        for _ in 0..self.count {
            let (new_remaining, new_result) = self.parser.parse(remaining)?;
            result.push(new_result);
            remaining = new_remaining;
        }
        Ok((remaining, result))
    }
}
/// Конструкция 'либо-либо'
enum Either<Left, Right> {
    Left(Left),
    Right(Right),
}

impl Parsable for AuthData {
    type Parser = Map<Take<stdp::Byte>, fn(Vec<u8>) -> Self>;

    fn parser() -> Self::Parser {
        map(take(AUTHDATA_SIZE, stdp::Byte), |authdata| {
            AuthData::new(authdata.try_into().unwrap_or([0; AUTHDATA_SIZE]))
        })
    }
}
/// Конструктор `Take`
fn take<T: for<'a> Parser<'a>>(count: usize, parser: T) -> Take<T> {
    Take { count, parser }
}

/// Статус, которые можно парсить
#[derive(Debug, PartialEq)]
pub enum Status {
    Ok,
    Err(String),
}
impl Parsable for Status {
    type Parser = Alt<(
        Map<Tag, fn(()) -> Self>,
        Map<Delimited<Tag, Unquote, Tag>, fn(Cow<str>) -> Self>,
    )>;

    fn parser() -> Self::Parser {
        fn to_ok(_: ()) -> Status {
            Status::Ok
        }
        fn to_err(error: Cow<str>) -> Status {
            Status::Err(error.into_owned())
        }
        alt2(
            map(tag("Ok"), to_ok),
            map(delimited(tag("Err("), unquote(), tag(")")), to_err),
        )
    }
}

/// Пара 'сокращённое название предмета' - 'его описание'
#[derive(Debug, Clone, PartialEq)]
pub struct AssetDsc {
    pub id: String,
    /// `dsc` aka `description`
    pub dsc: String,
}
impl Parsable for AssetDsc {
    type Parser = Map<
        Delimited<
            All<(StripWhitespace<Tag>, StripWhitespace<Tag>)>,
            Permutation<(KeyValue<Unquote>, KeyValue<Unquote>)>,
            StripWhitespace<Tag>,
        >,
        fn((Cow<str>, Cow<str>)) -> Self,
    >;

    fn parser() -> Self::Parser {
        // комбинаторы парсеров - это круто
        map(
            delimited(
                all2(
                    strip_whitespace(tag("AssetDsc")),
                    strip_whitespace(tag("{")),
                ),
                permutation2(
                    key_value("id", unquote()),
                    key_value("dsc", unquote()),
                ),
                strip_whitespace(tag("}")),
            ),
            |(id, dsc)| AssetDsc {
                id: id.into_owned(),
                dsc: dsc.into_owned(),
            },
        )
    }
}

impl Parsable for Bucket {
    type Parser = Map<
        Delimited<
            All<(StripWhitespace<Tag>, StripWhitespace<Tag>)>,
            Permutation<(KeyValue<ParseAssetIdentifier>, KeyValue<NonZeroU32>)>,
            StripWhitespace<Tag>,
        >,
        fn((AssetIdentifier, NonZeroU32)) -> Self,
    >;

    fn parser() -> Self::Parser {
        map(
            delimited(
                all2(
                    strip_whitespace(tag("Bucket")),
                    strip_whitespace(tag("{")),
                ),
                permutation2(
                    key_value("asset_id", asset_identifier()),
                    key_value("count", NonZeroU32::MIN),
                ),
                strip_whitespace(tag("}")),
            ),
            |(asset_id, count)| Bucket::new(asset_id, count.get()),
        )
    }
}

impl Parsable for UserCash {
    type Parser = Map<
        Delimited<
            All<(StripWhitespace<Tag>, StripWhitespace<Tag>)>,
            Permutation<(KeyValue<ParseUserId>, KeyValue<NonZeroU32>)>,
            StripWhitespace<Tag>,
        >,
        fn((UserId, NonZeroU32)) -> Self,
    >;

    fn parser() -> Self::Parser {
        map(
            delimited(
                all2(
                    strip_whitespace(tag("UserCash")),
                    strip_whitespace(tag("{")),
                ),
                permutation2(
                    key_value("user_id", user_id()),
                    key_value("count", NonZeroU32::MIN),
                ),
                strip_whitespace(tag("}")),
            ),
            |(user_id, count)| UserCash::new(user_id, count),
        )
    }
}

impl Parsable for UserBucket {
    type Parser = Map<
        Delimited<
            All<(StripWhitespace<Tag>, StripWhitespace<Tag>)>,
            Permutation<(
                KeyValue<ParseUserId>,
                KeyValue<<Bucket as Parsable>::Parser>,
            )>,
            StripWhitespace<Tag>,
        >,
        fn((UserId, Bucket)) -> Self,
    >;

    fn parser() -> Self::Parser {
        map(
            delimited(
                all2(
                    strip_whitespace(tag("UserBucket")),
                    strip_whitespace(tag("{")),
                ),
                permutation2(
                    key_value("user_id", user_id()),
                    key_value("Bucket", Bucket::parser()),
                ),
                strip_whitespace(tag("}")),
            ),
            |(user_id, bucket)| UserBucket::new(user_id, bucket),
        )
    }
}
impl Parsable for UserBuckets {
    type Parser = Map<
        Delimited<
            All<(StripWhitespace<Tag>, StripWhitespace<Tag>)>,
            Permutation<(
                KeyValue<ParseUserId>,
                KeyValue<List<<Bucket as Parsable>::Parser>>,
            )>,
            StripWhitespace<Tag>,
        >,
        fn((UserId, Vec<Bucket>)) -> Self,
    >;

    fn parser() -> Self::Parser {
        map(
            delimited(
                all2(
                    strip_whitespace(tag("UserBuckets")),
                    strip_whitespace(tag("{")),
                ),
                permutation2(
                    key_value("user_id", user_id()),
                    key_value("buckets", list(Bucket::parser())),
                ),
                strip_whitespace(tag("}")),
            ),
            |(user_id, buckets)| UserBuckets::new(user_id, buckets),
        )
    }
}
impl Parsable for Announcements {
    type Parser = Map<
        List<<UserBuckets as Parsable>::Parser>,
        fn(Vec<UserBuckets>) -> Self,
    >;

    fn parser() -> Self::Parser {
        fn from_vec(vec: Vec<UserBuckets>) -> Announcements {
            vec.into()
        }
        map(list(UserBuckets::parser()), from_vec)
    }
}

/// One generic parsing function
pub fn just_parse<T>(input: &str) -> Result<(&str, T), ParsingError>
where
    T: Parsable,
{
    T::parser().parse(input)
}

impl Parsable for SystemLogErrorKind {
    type Parser = Preceded<
        Tag,
        Alt<(
            Map<
                Preceded<StripWhitespace<Tag>, StripWhitespace<Unquote>>,
                fn(Cow<str>) -> SystemLogErrorKind,
            >,
            Map<
                Preceded<StripWhitespace<Tag>, StripWhitespace<Unquote>>,
                fn(Cow<str>) -> SystemLogErrorKind,
            >,
        )>,
    >;

    fn parser() -> Self::Parser {
        preceded(
            tag("Error"),
            alt2(
                map(
                    preceded(
                        strip_whitespace(tag("NetworkError")),
                        strip_whitespace(unquote()),
                    ),
                    |error: Cow<str>| {
                        SystemLogErrorKind::NetworkError(error.into_owned())
                    },
                ),
                map(
                    preceded(
                        strip_whitespace(tag("AccessDenied")),
                        strip_whitespace(unquote()),
                    ),
                    |error: Cow<str>| {
                        SystemLogErrorKind::AccessDenied(error.into_owned())
                    },
                ),
            ),
        )
    }
}
impl Parsable for SystemLogTraceKind {
    type Parser = Preceded<
        Tag,
        Alt<(
            Map<
                Preceded<StripWhitespace<Tag>, StripWhitespace<Unquote>>,
                fn(Cow<str>) -> SystemLogTraceKind,
            >,
            Map<
                Preceded<StripWhitespace<Tag>, StripWhitespace<Unquote>>,
                fn(Cow<str>) -> SystemLogTraceKind,
            >,
        )>,
    >;

    fn parser() -> Self::Parser {
        preceded(
            tag("Trace"),
            alt2(
                map(
                    preceded(
                        strip_whitespace(tag("SendRequest")),
                        strip_whitespace(unquote()),
                    ),
                    |request: Cow<str>| {
                        SystemLogTraceKind::SendRequest(request.into_owned())
                    },
                ),
                map(
                    preceded(
                        strip_whitespace(tag("GetResponse")),
                        strip_whitespace(unquote()),
                    ),
                    |response: Cow<str>| {
                        SystemLogTraceKind::GetResponse(response.into_owned())
                    },
                ),
            ),
        )
    }
}
impl Parsable for SystemLogKind {
    type Parser = StripWhitespace<
        Preceded<
            Tag,
            Alt<(
                Map<
                    <SystemLogTraceKind as Parsable>::Parser,
                    fn(SystemLogTraceKind) -> SystemLogKind,
                >,
                Map<
                    <SystemLogErrorKind as Parsable>::Parser,
                    fn(SystemLogErrorKind) -> SystemLogKind,
                >,
            )>,
        >,
    >;

    fn parser() -> Self::Parser {
        strip_whitespace(preceded(
            tag("System::"),
            alt2(
                map(SystemLogTraceKind::parser(), |trace| {
                    SystemLogKind::Trace(trace)
                }),
                map(SystemLogErrorKind::parser(), |error| {
                    SystemLogKind::Error(error)
                }),
            ),
        ))
    }
}
impl Parsable for AppLogErrorKind {
    type Parser = Preceded<
        Tag,
        Alt<(
            Map<
                Preceded<StripWhitespace<Tag>, StripWhitespace<Unquote>>,
                fn(Cow<str>) -> AppLogErrorKind,
            >,
            Map<
                Preceded<StripWhitespace<Tag>, StripWhitespace<Unquote>>,
                fn(Cow<str>) -> AppLogErrorKind,
            >,
        )>,
    >;

    fn parser() -> Self::Parser {
        preceded(
            tag("Error"),
            alt2(
                map(
                    preceded(
                        strip_whitespace(tag("LackOf")),
                        strip_whitespace(unquote()),
                    ),
                    |error: Cow<str>| {
                        AppLogErrorKind::LackOf(error.into_owned())
                    },
                ),
                map(
                    preceded(
                        strip_whitespace(tag("SystemError")),
                        strip_whitespace(unquote()),
                    ),
                    |error: Cow<str>| {
                        AppLogErrorKind::SystemError(error.into_owned())
                    },
                ),
            ),
        )
    }
}
impl Parsable for AppLogTraceKind {
    type Parser = Preceded<
        Tag,
        Alt<(
            Map<
                Preceded<
                    StripWhitespace<Tag>,
                    StripWhitespace<<AuthData as Parsable>::Parser>,
                >,
                fn(AuthData) -> AppLogTraceKind,
            >,
            Map<
                Preceded<StripWhitespace<Tag>, StripWhitespace<Unquote>>,
                fn(Cow<str>) -> AppLogTraceKind,
            >,
            Map<
                Preceded<
                    StripWhitespace<Tag>,
                    StripWhitespace<<Announcements as Parsable>::Parser>,
                >,
                fn(Announcements) -> AppLogTraceKind,
            >,
            Map<
                Preceded<StripWhitespace<Tag>, StripWhitespace<Unquote>>,
                fn(Cow<str>) -> AppLogTraceKind,
            >,
        )>,
    >;

    fn parser() -> Self::Parser {
        preceded(
            tag("Trace"),
            alt4(
                map(
                    preceded(
                        strip_whitespace(tag("Connect")),
                        strip_whitespace(AuthData::parser()),
                    ),
                    |authdata| AppLogTraceKind::Connect(authdata),
                ),
                map(
                    preceded(
                        strip_whitespace(tag("SendRequest")),
                        strip_whitespace(unquote()),
                    ),
                    |trace: Cow<str>| {
                        AppLogTraceKind::SendRequest(trace.into_owned())
                    },
                ),
                map(
                    preceded(
                        strip_whitespace(tag("Check")),
                        strip_whitespace(Announcements::parser()),
                    ),
                    |announcements| AppLogTraceKind::Check(announcements),
                ),
                map(
                    preceded(
                        strip_whitespace(tag("GetResponse")),
                        strip_whitespace(unquote()),
                    ),
                    |trace: Cow<str>| {
                        AppLogTraceKind::GetResponse(trace.into_owned())
                    },
                ),
            ),
        )
    }
}
impl Parsable for AppLogJournalKind {
    type Parser = Preceded<
        Tag,
        Alt<(
            Map<
                Preceded<
                    StripWhitespace<Tag>,
                    Delimited<
                        Tag,
                        Permutation<(
                            KeyValue<ParseUserId>,
                            KeyValue<NonZeroU32>,
                        )>,
                        Tag,
                    >,
                >,
                fn((UserId, NonZeroU32)) -> AppLogJournalKind,
            >,
            Map<
                Preceded<
                    StripWhitespace<Tag>,
                    Delimited<Tag, KeyValue<ParseUserId>, Tag>,
                >,
                fn(UserId) -> AppLogJournalKind,
            >,
            Map<
                Preceded<
                    StripWhitespace<Tag>,
                    Delimited<
                        Tag,
                        Permutation<(
                            KeyValue<ParseAssetIdentifier>,
                            KeyValue<ParseUserId>,
                            KeyValue<NonZeroU32>,
                        )>,
                        Tag,
                    >,
                >,
                fn((AssetIdentifier, UserId, NonZeroU32)) -> AppLogJournalKind,
            >,
            Map<
                Preceded<
                    StripWhitespace<Tag>,
                    Delimited<
                        Tag,
                        Permutation<(
                            KeyValue<ParseAssetIdentifier>,
                            KeyValue<ParseUserId>,
                        )>,
                        Tag,
                    >,
                >,
                fn((AssetIdentifier, UserId)) -> AppLogJournalKind,
            >,
            Map<
                Preceded<StripWhitespace<Tag>, <UserCash as Parsable>::Parser>,
                fn(UserCash) -> AppLogJournalKind,
            >,
            Map<
                Preceded<StripWhitespace<Tag>, <UserCash as Parsable>::Parser>,
                fn(UserCash) -> AppLogJournalKind,
            >,
            Map<
                Preceded<
                    StripWhitespace<Tag>,
                    <UserBucket as Parsable>::Parser,
                >,
                fn(UserBucket) -> AppLogJournalKind,
            >,
            Map<
                Preceded<
                    StripWhitespace<Tag>,
                    <UserBucket as Parsable>::Parser,
                >,
                fn(UserBucket) -> AppLogJournalKind,
            >,
        )>,
    >;

    fn parser() -> Self::Parser {
        preceded(
            tag("Journal"),
            alt8(
                map(
                    preceded(
                        strip_whitespace(tag("CreateUser")),
                        delimited(
                            tag("{"),
                            permutation2(
                                key_value("user_id", user_id()),
                                key_value(
                                    "authorized_capital",
                                    NonZeroU32::MIN,
                                ),
                            ),
                            tag("}"),
                        ),
                    ),
                    |(user_id, authorized_capital): (UserId, NonZeroU32)| {
                        AppLogJournalKind::CreateUser {
                            user_id,
                            authorized_capital,
                        }
                    },
                ),
                map(
                    preceded(
                        strip_whitespace(tag("DeleteUser")),
                        delimited(
                            tag("{"),
                            key_value("user_id", user_id()),
                            tag("}"),
                        ),
                    ),
                    |user_id: UserId| AppLogJournalKind::DeleteUser { user_id },
                ),
                map(
                    preceded(
                        strip_whitespace(tag("RegisterAsset")),
                        delimited(
                            tag("{"),
                            permutation3(
                                key_value("asset_id", asset_identifier()),
                                key_value("user_id", user_id()),
                                key_value("liquidity", NonZeroU32::MIN),
                            ),
                            tag("}"),
                        ),
                    ),
                    |(asset_id, user_id, liquidity): (
                        AssetIdentifier,
                        UserId,
                        NonZeroU32,
                    )| {
                        AppLogJournalKind::RegisterAsset {
                            asset_id,
                            user_id,
                            liquidity,
                        }
                    },
                ),
                map(
                    preceded(
                        strip_whitespace(tag("UnregisterAsset")),
                        delimited(
                            tag("{"),
                            permutation2(
                                key_value("asset_id", asset_identifier()),
                                key_value("user_id", user_id()),
                            ),
                            tag("}"),
                        ),
                    ),
                    |(asset_id, user_id): (AssetIdentifier, UserId)| {
                        AppLogJournalKind::UnregisterAsset { asset_id, user_id }
                    },
                ),
                map(
                    preceded(
                        strip_whitespace(tag("DepositCash")),
                        UserCash::parser(),
                    ),
                    |user_cash| AppLogJournalKind::DepositCash(user_cash),
                ),
                map(
                    preceded(
                        strip_whitespace(tag("WithdrawCash")),
                        UserCash::parser(),
                    ),
                    |user_cash| AppLogJournalKind::WithdrawCash(user_cash),
                ),
                map(
                    preceded(
                        strip_whitespace(tag("BuyAsset")),
                        UserBucket::parser(),
                    ),
                    |user_bucket| AppLogJournalKind::BuyAsset(user_bucket),
                ),
                map(
                    preceded(
                        strip_whitespace(tag("SellAsset")),
                        UserBucket::parser(),
                    ),
                    |user_bucket| AppLogJournalKind::SellAsset(user_bucket),
                ),
            ),
        )
    }
}
impl Parsable for AppLogKind {
    type Parser = StripWhitespace<
        Preceded<
            Tag,
            Alt<(
                Map<
                    <AppLogErrorKind as Parsable>::Parser,
                    fn(AppLogErrorKind) -> AppLogKind,
                >,
                Map<
                    <AppLogTraceKind as Parsable>::Parser,
                    fn(AppLogTraceKind) -> AppLogKind,
                >,
                Map<
                    <AppLogJournalKind as Parsable>::Parser,
                    fn(AppLogJournalKind) -> AppLogKind,
                >,
            )>,
        >,
    >;

    fn parser() -> Self::Parser {
        strip_whitespace(preceded(
            tag("App::"),
            alt3(
                map(AppLogErrorKind::parser(), |error| {
                    AppLogKind::Error(error)
                }),
                map(AppLogTraceKind::parser(), |trace| {
                    AppLogKind::Trace(trace)
                }),
                map(AppLogJournalKind::parser(), |journal| {
                    AppLogKind::Journal(journal)
                }),
            ),
        ))
    }
}
impl Parsable for LogKind {
    type Parser = StripWhitespace<
        Alt<(
            Map<
                <SystemLogKind as Parsable>::Parser,
                fn(SystemLogKind) -> LogKind,
            >,
            Map<<AppLogKind as Parsable>::Parser, fn(AppLogKind) -> LogKind>,
        )>,
    >;

    fn parser() -> Self::Parser {
        strip_whitespace(alt2(
            map(SystemLogKind::parser(), |system| LogKind::System(system)),
            map(AppLogKind::parser(), |app| LogKind::App(app)),
        ))
    }
}
impl Parsable for LogLine {
    type Parser = Map<
        All<(
            <LogKind as Parsable>::Parser,
            StripWhitespace<Preceded<Tag, NonZeroU32>>,
        )>,
        fn((LogKind, NonZeroU32)) -> Self,
    >;

    fn parser() -> Self::Parser {
        map(
            all2(
                LogKind::parser(),
                strip_whitespace(preceded(tag("requestid="), NonZeroU32::MIN)),
            ),
            |(kind, request_id)| LogLine::new(kind, request_id.get()),
        )
    }
}

/// Парсер строки логов
pub struct LogLineParser {
    parser: std::sync::OnceLock<<LogLine as Parsable>::Parser>,
}
impl LogLineParser {
    pub fn parse<'a>(
        &self,
        input: &'a str,
    ) -> Result<(&'a str, LogLine), ParsingError> {
        self.parser
            .get_or_init(|| <LogLine as Parsable>::parser())
            .parse(input)
    }
}
// подсказка: singleton, без которого можно обойтись
// парсеры не страшно вытащить в pub
/// Единожды собранный парсер логов
pub static LOG_LINE_PARSER: LogLineParser = LogLineParser {
    parser: std::sync::OnceLock::new(),
};

#[cfg(test)]
mod test {
    use std::num::NonZeroI32;

    use super::*;

    #[test]
    fn test_u32() {
        assert_eq!(
            NonZeroU32::MIN.parse("411"),
            Ok(("", NonZeroU32::new(411).unwrap()))
        );
        assert_eq!(
            NonZeroU32::MIN.parse("411ab"),
            Ok(("ab", NonZeroU32::new(411).unwrap()))
        );
        assert!(NonZeroU32::MIN.parse("").is_err());
        assert!(NonZeroU32::MIN.parse("-3").is_err());
        assert_eq!(
            NonZeroU32::MIN.parse("0x03"),
            Ok(("", NonZeroU32::new(0x3).unwrap()))
        );
        assert_eq!(
            NonZeroU32::MIN.parse("0x03abg"),
            Ok(("g", NonZeroU32::new(0x3ab).unwrap()))
        );
        assert!(NonZeroU32::MIN.parse("0x").is_err());
        assert_eq!(
            NonZeroU32::MIN.parse("0"),
            Err(ParsingError::ParseNonZeroIntError)
        );
    }

    #[test]
    fn test_i32() {
        assert_eq!(
            NonZeroI32::MIN.parse("411"),
            Ok(("", NonZeroI32::new(411).unwrap()))
        );
        assert_eq!(
            NonZeroI32::MIN.parse("411ab"),
            Ok(("ab", NonZeroI32::new(411).unwrap()))
        );
        assert!(NonZeroI32::MIN.parse("").is_err());
        assert_eq!(
            NonZeroI32::MIN.parse("-3"),
            Ok(("", NonZeroI32::new(-3).unwrap()))
        );
        assert!(NonZeroI32::MIN.parse("0x03").is_err());
        assert!(NonZeroI32::MIN.parse("-").is_err());
    }

    #[test]
    fn test_quote() {
        assert_eq!(quote(r#"411"#), r#""411""#.to_string());
        assert_eq!(quote(r#"4\11""#), r#""4\\11\"""#.to_string());
    }

    #[test]
    fn test_do_unquote_non_escaped() {
        assert_eq!(do_unquote_non_escaped(r#""411""#), Ok(("", "411")));
        assert!(do_unquote_non_escaped(r#" "411""#).is_err());
        assert!(do_unquote_non_escaped(r#"411"#).is_err());
    }

    #[test]
    fn test_unquote() {
        assert_eq!(Unquote.parse(r#""411""#), Ok(("", Cow::Borrowed("411"))));
        assert!(Unquote.parse(r#" "411""#).is_err());
        assert!(Unquote.parse(r#"411"#).is_err());

        assert_eq!(
            Unquote.parse(r#""ni\\c\"e""#),
            Ok(("", Cow::Borrowed(r#"ni\c"e"#)))
        );
    }

    #[test]
    fn test_tag() {
        assert_eq!(tag("key=").parse("key=value"), Ok(("value", ())));
        assert!(tag("key=").parse("key:value").is_err());
    }

    #[test]
    fn test_quoted_tag() {
        assert_eq!(
            quoted_tag("key").parse(r#""key"=value"#),
            Ok(("=value", ()))
        );
        assert!(quoted_tag("key").parse(r#""key:"value"#).is_err());
        assert!(quoted_tag("key").parse(r#"key=value"#).is_err());
    }

    #[test]
    fn test_strip_whitespace() {
        assert_eq!(
            strip_whitespace(tag("hello")).parse(" hello world"),
            Ok(("world", ()))
        );
        assert_eq!(strip_whitespace(tag("hello")).parse("hello"), Ok(("", ())));
        assert_eq!(
            strip_whitespace(NonZeroU32::MIN).parse(" 42 answer"),
            Ok(("answer", NonZeroU32::new(42).unwrap()))
        );
    }

    #[test]
    fn test_delimited() {
        assert_eq!(
            delimited(tag("["), NonZeroU32::MIN, tag("]")).parse("[0x32]"),
            Ok(("", NonZeroU32::new(0x32).unwrap()))
        );
        assert_eq!(
            delimited(tag("["), NonZeroU32::MIN, tag("]")).parse("[0x32] nice"),
            Ok((" nice", NonZeroU32::new(0x32).unwrap()))
        );
        assert!(
            delimited(tag("["), NonZeroU32::MIN, tag("]"))
                .parse("0x32]")
                .is_err()
        );
        assert!(
            delimited(tag("["), NonZeroU32::MIN, tag("]"))
                .parse("[0x32")
                .is_err()
        );
    }

    #[test]
    fn test_key_value() {
        assert_eq!(
            key_value("key", NonZeroU32::MIN).parse(r#""key":32,"#),
            Ok(("", NonZeroU32::new(32).unwrap()))
        );
        assert!(
            key_value("key", NonZeroU32::MIN)
                .parse(r#"key:32,"#)
                .is_err()
        );
        assert!(
            key_value("key", NonZeroU32::MIN)
                .parse(r#""key":32"#)
                .is_err()
        );
        assert_eq!(
            key_value("key", NonZeroU32::MIN).parse(r#" "key" : 32 , nice"#),
            Ok(("nice", NonZeroU32::new(32).unwrap()))
        );
    }

    #[test]
    fn test_list() {
        assert_eq!(
            list(NonZeroU32::MIN).parse("[1,2,3,4,]"),
            Ok((
                "",
                vec![
                    NonZeroU32::new(1).unwrap(),
                    NonZeroU32::new(2).unwrap(),
                    NonZeroU32::new(3).unwrap(),
                    NonZeroU32::new(4).unwrap(),
                ]
            ))
        );
        assert_eq!(
            list(NonZeroU32::MIN).parse(" [ 1 , 2 , 3 , 4 , ] nice"),
            Ok((
                "nice",
                vec![
                    NonZeroU32::new(1).unwrap(),
                    NonZeroU32::new(2).unwrap(),
                    NonZeroU32::new(3).unwrap(),
                    NonZeroU32::new(4).unwrap(),
                ]
            ))
        );
        assert!(list(NonZeroU32::MIN).parse("1,2,3,4,").is_err());
        assert_eq!(list(NonZeroU32::MIN).parse("[]"), Ok(("", vec![])));
    }

    #[test]
    fn test_authdata() {
        let s = "30c305825b900077ae7f8259c1c328aa3e124a07f3bfbbf216dfc6e308beea6e474b9a7ea6c24d003a6ae4fcf04a9e6ef7c7f17cdaa0296f66a88036badcf01f053da806fad356546349deceff24621b895440d05a715b221af8e9e068073d6dec04f148175717d3c2d1b6af84e2375718ab4a1eba7e037c1c1d43b4cf422d6f2aa9194266f0a7544eaeff8167f0e993d0ea6a8ddb98bfeb8805635d5ea9f6592fd5297e6f83b6834190f99449722cd0de87a4c122f08bbe836fd3092e5f0d37a3057e90f3dd41048da66cad3e8fd3ef72a9d86ecd9009c2db996af29dc62af5ef5eb04d0e16ce8fcecba92a4a9888f52d5d575e7dbc302ed97dbf69df15bb4f5c5601d38fbe3bd89d88768a6aed11ce2f95a6ad30bb72e787bfb734701cea1f38168be44ea19d3e98dd3c953fdb9951ac9c6e221bb0f980d8f0952ac8127da5bda7077dd25ffc8e1515c529f29516dacec6be9c084e6c91698267b2aed9038eca5ebafad479c5fb17652e25bb5b85586fae645bd7c3253d9916c0af65a20253412d5484ac15d288c6ca8823469090ded5ce0975dada63653797129f0e926af6247b457b067db683e37d848e0acf30e5602b78f1848e8da4b640ed08b75f3519a40ec96b2be964234beab37759504376c6e5ebfacdc57e4c7a22cf1e879d7bde29a2dca5fe20420215b59d102fd016606c533e8e36f7da114910664bade9b295d9043a01bc0dc4d8abbc16b1cec7789d89e699ad99dae597c7f10d6f047efc011d67444695cb8e6e8b3dba17ccc693729d01312d0f12a3fc76e12c2e4984af5cb3049b9d8a13124a1f770e96bae1fb153ba4c91bea4fae6f03010275d5a9b14012bdd678e037934dc6762005de54b32a7684e03060d5cc80378e9bef05b8f0692202944401bd06e4553e4490a0e57c5a72fc8abb1f714e22ea950fb2f1de284d6ff3da435954de355c677f60db4252a510919cbe7dadfed0441cf125fd8894753af8114f2ddacb75c3daa460920fc47d285e59fe9110e4151fcef03fa246cd2dd9a4d573e1dbbda1c6968cf4f546289b95ce1bf0a55eea6531382826d4002bc46bf441ce16056d42b5a2079e299e3191c23a7604cde03de6081e06f93cfe632c9a6088cd328662d47a4954934832df5b5f3765dbe136114c73c55cb7ce639e5d40d1d1d8f540d3c8e1bc7423f032c0da5264353468f009c973eec0448e41f9289e8d9dadc68da77d3c3ab3a6477d44024f21fba0bd4477d81c6027657527aa0413b45f417cb7b3beea835a1d5d795414d38156324cb5c1303e9924dbe40cd497c4c23c221cb912058c939bea8b79b3fea360fecaa83375a9a84e338d9e863e8021ad2df4430b8dea0c1714e1bdc478f559705549ad738453ab65c0ffcc8cf0e3bafaf4afad75ecc4dfad0de0cfe27d50d656456ea6c361b76508357714079424";
        let res = AuthData::parser().parse(s);
        assert!(res.is_ok());
        assert_eq!(res.as_ref().unwrap().0.len(), 0);
    }

    #[test]
    fn test_asset_dsc() {
        assert_eq!(
            all2(
                strip_whitespace(tag("AssetDsc")),
                strip_whitespace(tag("{"))
            )
            .parse(" AssetDsc { "),
            Ok(("", ((), ())))
        );

        assert_eq!(
            AssetDsc::parser()
                .parse(r#"AssetDsc{"id":"usd","dsc":"USA dollar",}"#),
            Ok((
                "",
                AssetDsc {
                    id: "usd".into(),
                    dsc: "USA dollar".into()
                }
            ))
        );
        assert_eq!(
            AssetDsc::parser().parse(
                r#" AssetDsc { "id" : "usd" , "dsc" : "USA dollar" , } "#
            ),
            Ok((
                "",
                AssetDsc {
                    id: "usd".into(),
                    dsc: "USA dollar".into()
                }
            ))
        );
        assert_eq!(
            AssetDsc::parser().parse(
                r#" AssetDsc { "id" : "usd" , "dsc" : "USA dollar" , } nice "#
            ),
            Ok((
                "nice ",
                AssetDsc {
                    id: "usd".into(),
                    dsc: "USA dollar".into()
                }
            ))
        );

        assert_eq!(
            AssetDsc::parser()
                .parse(r#"AssetDsc{"dsc":"USA dollar","id":"usd",}"#),
            Ok((
                "",
                AssetDsc {
                    id: "usd".into(),
                    dsc: "USA dollar".into()
                }
            ))
        );
    }

    #[test]
    fn test_bucket() {
        assert_eq!(
            Bucket::parser().parse(r#"Bucket{"asset_id":"usd","count":42,}"#),
            Ok(("", Bucket::new("usd".parse().unwrap(), 42)))
        );
        assert_eq!(
            Bucket::parser().parse(r#"Bucket{"count":42,"asset_id":"usd",}"#),
            Ok(("", Bucket::new("usd".parse().unwrap(), 42)))
        );
    }

    #[test]
    fn test_log_kind() {
        assert_eq!(
            preceded(
                strip_whitespace(tag("NetworkError")),
                strip_whitespace(unquote())
            )
            .parse(r#"NetworkError "url unknown""#),
            Ok(("", Cow::Borrowed("url unknown")))
        );

        assert_eq!(
            LogKind::parser()
                .parse(r#"System::Error NetworkError "url unknown""#),
            Ok((
                "",
                LogKind::System(SystemLogKind::Error(
                    SystemLogErrorKind::NetworkError("url unknown".into())
                ))
            ))
        );
        assert_eq!(LogKind::parser().parse(r#"App::Trace Connect 30c305825b900077ae7f8259c1c328aa3e124a07f3bfbbf216dfc6e308beea6e474b9a7ea6c24d003a6ae4fcf04a9e6ef7c7f17cdaa0296f66a88036badcf01f053da806fad356546349deceff24621b895440d05a715b221af8e9e068073d6dec04f148175717d3c2d1b6af84e2375718ab4a1eba7e037c1c1d43b4cf422d6f2aa9194266f0a7544eaeff8167f0e993d0ea6a8ddb98bfeb8805635d5ea9f6592fd5297e6f83b6834190f99449722cd0de87a4c122f08bbe836fd3092e5f0d37a3057e90f3dd41048da66cad3e8fd3ef72a9d86ecd9009c2db996af29dc62af5ef5eb04d0e16ce8fcecba92a4a9888f52d5d575e7dbc302ed97dbf69df15bb4f5c5601d38fbe3bd89d88768a6aed11ce2f95a6ad30bb72e787bfb734701cea1f38168be44ea19d3e98dd3c953fdb9951ac9c6e221bb0f980d8f0952ac8127da5bda7077dd25ffc8e1515c529f29516dacec6be9c084e6c91698267b2aed9038eca5ebafad479c5fb17652e25bb5b85586fae645bd7c3253d9916c0af65a20253412d5484ac15d288c6ca8823469090ded5ce0975dada63653797129f0e926af6247b457b067db683e37d848e0acf30e5602b78f1848e8da4b640ed08b75f3519a40ec96b2be964234beab37759504376c6e5ebfacdc57e4c7a22cf1e879d7bde29a2dca5fe20420215b59d102fd016606c533e8e36f7da114910664bade9b295d9043a01bc0dc4d8abbc16b1cec7789d89e699ad99dae597c7f10d6f047efc011d67444695cb8e6e8b3dba17ccc693729d01312d0f12a3fc76e12c2e4984af5cb3049b9d8a13124a1f770e96bae1fb153ba4c91bea4fae6f03010275d5a9b14012bdd678e037934dc6762005de54b32a7684e03060d5cc80378e9bef05b8f0692202944401bd06e4553e4490a0e57c5a72fc8abb1f714e22ea950fb2f1de284d6ff3da435954de355c677f60db4252a510919cbe7dadfed0441cf125fd8894753af8114f2ddacb75c3daa460920fc47d285e59fe9110e4151fcef03fa246cd2dd9a4d573e1dbbda1c6968cf4f546289b95ce1bf0a55eea6531382826d4002bc46bf441ce16056d42b5a2079e299e3191c23a7604cde03de6081e06f93cfe632c9a6088cd328662d47a4954934832df5b5f3765dbe136114c73c55cb7ce639e5d40d1d1d8f540d3c8e1bc7423f032c0da5264353468f009c973eec0448e41f9289e8d9dadc68da77d3c3ab3a6477d44024f21fba0bd4477d81c6027657527aa0413b45f417cb7b3beea835a1d5d795414d38156324cb5c1303e9924dbe40cd497c4c23c221cb912058c939bea8b79b3fea360fecaa83375a9a84e338d9e863e8021ad2df4430b8dea0c1714e1bdc478f559705549ad738453ab65c0ffcc8cf0e3bafaf4afad75ecc4dfad0de0cfe27d50d656456ea6c361b76508357714079424"#), Ok(("", LogKind::App(AppLogKind::Trace(AppLogTraceKind::Connect(AuthData::new([0x30,0xc3,0x05,0x82,0x5b,0x90,0x00,0x77,0xae,0x7f,0x82,0x59,0xc1,0xc3,0x28,0xaa,0x3e,0x12,0x4a,0x07,0xf3,0xbf,0xbb,0xf2,0x16,0xdf,0xc6,0xe3,0x08,0xbe,0xea,0x6e,0x47,0x4b,0x9a,0x7e,0xa6,0xc2,0x4d,0x00,0x3a,0x6a,0xe4,0xfc,0xf0,0x4a,0x9e,0x6e,0xf7,0xc7,0xf1,0x7c,0xda,0xa0,0x29,0x6f,0x66,0xa8,0x80,0x36,0xba,0xdc,0xf0,0x1f,0x05,0x3d,0xa8,0x06,0xfa,0xd3,0x56,0x54,0x63,0x49,0xde,0xce,0xff,0x24,0x62,0x1b,0x89,0x54,0x40,0xd0,0x5a,0x71,0x5b,0x22,0x1a,0xf8,0xe9,0xe0,0x68,0x07,0x3d,0x6d,0xec,0x04,0xf1,0x48,0x17,0x57,0x17,0xd3,0xc2,0xd1,0xb6,0xaf,0x84,0xe2,0x37,0x57,0x18,0xab,0x4a,0x1e,0xba,0x7e,0x03,0x7c,0x1c,0x1d,0x43,0xb4,0xcf,0x42,0x2d,0x6f,0x2a,0xa9,0x19,0x42,0x66,0xf0,0xa7,0x54,0x4e,0xae,0xff,0x81,0x67,0xf0,0xe9,0x93,0xd0,0xea,0x6a,0x8d,0xdb,0x98,0xbf,0xeb,0x88,0x05,0x63,0x5d,0x5e,0xa9,0xf6,0x59,0x2f,0xd5,0x29,0x7e,0x6f,0x83,0xb6,0x83,0x41,0x90,0xf9,0x94,0x49,0x72,0x2c,0xd0,0xde,0x87,0xa4,0xc1,0x22,0xf0,0x8b,0xbe,0x83,0x6f,0xd3,0x09,0x2e,0x5f,0x0d,0x37,0xa3,0x05,0x7e,0x90,0xf3,0xdd,0x41,0x04,0x8d,0xa6,0x6c,0xad,0x3e,0x8f,0xd3,0xef,0x72,0xa9,0xd8,0x6e,0xcd,0x90,0x09,0xc2,0xdb,0x99,0x6a,0xf2,0x9d,0xc6,0x2a,0xf5,0xef,0x5e,0xb0,0x4d,0x0e,0x16,0xce,0x8f,0xce,0xcb,0xa9,0x2a,0x4a,0x98,0x88,0xf5,0x2d,0x5d,0x57,0x5e,0x7d,0xbc,0x30,0x2e,0xd9,0x7d,0xbf,0x69,0xdf,0x15,0xbb,0x4f,0x5c,0x56,0x01,0xd3,0x8f,0xbe,0x3b,0xd8,0x9d,0x88,0x76,0x8a,0x6a,0xed,0x11,0xce,0x2f,0x95,0xa6,0xad,0x30,0xbb,0x72,0xe7,0x87,0xbf,0xb7,0x34,0x70,0x1c,0xea,0x1f,0x38,0x16,0x8b,0xe4,0x4e,0xa1,0x9d,0x3e,0x98,0xdd,0x3c,0x95,0x3f,0xdb,0x99,0x51,0xac,0x9c,0x6e,0x22,0x1b,0xb0,0xf9,0x80,0xd8,0xf0,0x95,0x2a,0xc8,0x12,0x7d,0xa5,0xbd,0xa7,0x07,0x7d,0xd2,0x5f,0xfc,0x8e,0x15,0x15,0xc5,0x29,0xf2,0x95,0x16,0xda,0xce,0xc6,0xbe,0x9c,0x08,0x4e,0x6c,0x91,0x69,0x82,0x67,0xb2,0xae,0xd9,0x03,0x8e,0xca,0x5e,0xba,0xfa,0xd4,0x79,0xc5,0xfb,0x17,0x65,0x2e,0x25,0xbb,0x5b,0x85,0x58,0x6f,0xae,0x64,0x5b,0xd7,0xc3,0x25,0x3d,0x99,0x16,0xc0,0xaf,0x65,0xa2,0x02,0x53,0x41,0x2d,0x54,0x84,0xac,0x15,0xd2,0x88,0xc6,0xca,0x88,0x23,0x46,0x90,0x90,0xde,0xd5,0xce,0x09,0x75,0xda,0xda,0x63,0x65,0x37,0x97,0x12,0x9f,0x0e,0x92,0x6a,0xf6,0x24,0x7b,0x45,0x7b,0x06,0x7d,0xb6,0x83,0xe3,0x7d,0x84,0x8e,0x0a,0xcf,0x30,0xe5,0x60,0x2b,0x78,0xf1,0x84,0x8e,0x8d,0xa4,0xb6,0x40,0xed,0x08,0xb7,0x5f,0x35,0x19,0xa4,0x0e,0xc9,0x6b,0x2b,0xe9,0x64,0x23,0x4b,0xea,0xb3,0x77,0x59,0x50,0x43,0x76,0xc6,0xe5,0xeb,0xfa,0xcd,0xc5,0x7e,0x4c,0x7a,0x22,0xcf,0x1e,0x87,0x9d,0x7b,0xde,0x29,0xa2,0xdc,0xa5,0xfe,0x20,0x42,0x02,0x15,0xb5,0x9d,0x10,0x2f,0xd0,0x16,0x60,0x6c,0x53,0x3e,0x8e,0x36,0xf7,0xda,0x11,0x49,0x10,0x66,0x4b,0xad,0xe9,0xb2,0x95,0xd9,0x04,0x3a,0x01,0xbc,0x0d,0xc4,0xd8,0xab,0xbc,0x16,0xb1,0xce,0xc7,0x78,0x9d,0x89,0xe6,0x99,0xad,0x99,0xda,0xe5,0x97,0xc7,0xf1,0x0d,0x6f,0x04,0x7e,0xfc,0x01,0x1d,0x67,0x44,0x46,0x95,0xcb,0x8e,0x6e,0x8b,0x3d,0xba,0x17,0xcc,0xc6,0x93,0x72,0x9d,0x01,0x31,0x2d,0x0f,0x12,0xa3,0xfc,0x76,0xe1,0x2c,0x2e,0x49,0x84,0xaf,0x5c,0xb3,0x04,0x9b,0x9d,0x8a,0x13,0x12,0x4a,0x1f,0x77,0x0e,0x96,0xba,0xe1,0xfb,0x15,0x3b,0xa4,0xc9,0x1b,0xea,0x4f,0xae,0x6f,0x03,0x01,0x02,0x75,0xd5,0xa9,0xb1,0x40,0x12,0xbd,0xd6,0x78,0xe0,0x37,0x93,0x4d,0xc6,0x76,0x20,0x05,0xde,0x54,0xb3,0x2a,0x76,0x84,0xe0,0x30,0x60,0xd5,0xcc,0x80,0x37,0x8e,0x9b,0xef,0x05,0xb8,0xf0,0x69,0x22,0x02,0x94,0x44,0x01,0xbd,0x06,0xe4,0x55,0x3e,0x44,0x90,0xa0,0xe5,0x7c,0x5a,0x72,0xfc,0x8a,0xbb,0x1f,0x71,0x4e,0x22,0xea,0x95,0x0f,0xb2,0xf1,0xde,0x28,0x4d,0x6f,0xf3,0xda,0x43,0x59,0x54,0xde,0x35,0x5c,0x67,0x7f,0x60,0xdb,0x42,0x52,0xa5,0x10,0x91,0x9c,0xbe,0x7d,0xad,0xfe,0xd0,0x44,0x1c,0xf1,0x25,0xfd,0x88,0x94,0x75,0x3a,0xf8,0x11,0x4f,0x2d,0xda,0xcb,0x75,0xc3,0xda,0xa4,0x60,0x92,0x0f,0xc4,0x7d,0x28,0x5e,0x59,0xfe,0x91,0x10,0xe4,0x15,0x1f,0xce,0xf0,0x3f,0xa2,0x46,0xcd,0x2d,0xd9,0xa4,0xd5,0x73,0xe1,0xdb,0xbd,0xa1,0xc6,0x96,0x8c,0xf4,0xf5,0x46,0x28,0x9b,0x95,0xce,0x1b,0xf0,0xa5,0x5e,0xea,0x65,0x31,0x38,0x28,0x26,0xd4,0x00,0x2b,0xc4,0x6b,0xf4,0x41,0xce,0x16,0x05,0x6d,0x42,0xb5,0xa2,0x07,0x9e,0x29,0x9e,0x31,0x91,0xc2,0x3a,0x76,0x04,0xcd,0xe0,0x3d,0xe6,0x08,0x1e,0x06,0xf9,0x3c,0xfe,0x63,0x2c,0x9a,0x60,0x88,0xcd,0x32,0x86,0x62,0xd4,0x7a,0x49,0x54,0x93,0x48,0x32,0xdf,0x5b,0x5f,0x37,0x65,0xdb,0xe1,0x36,0x11,0x4c,0x73,0xc5,0x5c,0xb7,0xce,0x63,0x9e,0x5d,0x40,0xd1,0xd1,0xd8,0xf5,0x40,0xd3,0xc8,0xe1,0xbc,0x74,0x23,0xf0,0x32,0xc0,0xda,0x52,0x64,0x35,0x34,0x68,0xf0,0x09,0xc9,0x73,0xee,0xc0,0x44,0x8e,0x41,0xf9,0x28,0x9e,0x8d,0x9d,0xad,0xc6,0x8d,0xa7,0x7d,0x3c,0x3a,0xb3,0xa6,0x47,0x7d,0x44,0x02,0x4f,0x21,0xfb,0xa0,0xbd,0x44,0x77,0xd8,0x1c,0x60,0x27,0x65,0x75,0x27,0xaa,0x04,0x13,0xb4,0x5f,0x41,0x7c,0xb7,0xb3,0xbe,0xea,0x83,0x5a,0x1d,0x5d,0x79,0x54,0x14,0xd3,0x81,0x56,0x32,0x4c,0xb5,0xc1,0x30,0x3e,0x99,0x24,0xdb,0xe4,0x0c,0xd4,0x97,0xc4,0xc2,0x3c,0x22,0x1c,0xb9,0x12,0x05,0x8c,0x93,0x9b,0xea,0x8b,0x79,0xb3,0xfe,0xa3,0x60,0xfe,0xca,0xa8,0x33,0x75,0xa9,0xa8,0x4e,0x33,0x8d,0x9e,0x86,0x3e,0x80,0x21,0xad,0x2d,0xf4,0x43,0x0b,0x8d,0xea,0x0c,0x17,0x14,0xe1,0xbd,0xc4,0x78,0xf5,0x59,0x70,0x55,0x49,0xad,0x73,0x84,0x53,0xab,0x65,0xc0,0xff,0xcc,0x8c,0xf0,0xe3,0xba,0xfa,0xf4,0xaf,0xad,0x75,0xec,0xc4,0xdf,0xad,0x0d,0xe0,0xcf,0xe2,0x7d,0x50,0xd6,0x56,0x45,0x6e,0xa6,0xc3,0x61,0xb7,0x65,0x08,0x35,0x77,0x14,0x07,0x94,0x24])))))));
        assert_eq!(
            LogKind::parser().parse(
                r#"App::Journal CreateUser {"user_id": "Steeve", "authorized_capital": 10000,}"#
            ),
            Ok((
                "",
                LogKind::App(AppLogKind::Journal(AppLogJournalKind::CreateUser {
                    user_id: "Steeve".parse().unwrap(),
                    authorized_capital: NonZeroU32::new(10_000).unwrap()
                }))
            ))
        );
        assert_eq!(
            LogKind::parser()
                .parse(r#"App::Journal DeleteUser {"user_id": "Steeve",}"#),
            Ok((
                "",
                LogKind::App(AppLogKind::Journal(
                    AppLogJournalKind::DeleteUser {
                        user_id: "Steeve".parse().unwrap()
                    }
                ))
            ))
        );
        assert_eq!(LogKind::parser().parse(r#"App::Journal RegisterAsset {"asset_id": "bayc", "liquidity": 100000000, "user_id": "Steeve",}"#), Ok(("", LogKind::App(AppLogKind::Journal(AppLogJournalKind::RegisterAsset{asset_id: "bayc".parse().unwrap(), user_id: "Steeve".parse().unwrap(), liquidity: NonZeroU32::new(100_000_000).unwrap()})))));
        assert_eq!(
            LogKind::parser().parse(
                r#"App::Journal DepositCash UserCash{"user_id": "Steeve", "count": 10,}"#
            ),
            Ok((
                "",
                LogKind::App(AppLogKind::Journal(AppLogJournalKind::DepositCash(
                    UserCash::new(
                        "Steeve".parse().unwrap(),
                        NonZeroU32::new(10).unwrap()
                    )
                )))
            ))
        );
        assert_eq!(LogKind::parser().parse(r#"App::Journal BuyAsset UserBucket{"user_id": "Steeve", "Bucket": Bucket{"asset_id":"bayc","count":1,},}"#), Ok(("", LogKind::App(AppLogKind::Journal(AppLogJournalKind::BuyAsset(UserBucket::new("Steeve".parse().unwrap(), Bucket::new("bayc".parse().unwrap(), 1))))))));
    }

    // ----- do_unquote / Unquote edge cases -----

    #[test]
    fn test_do_unquote_no_leading_quote() {
        assert_eq!(
            do_unquote("no quote here"),
            Err(ParsingError::ParseQuotedString)
        );
    }

    #[test]
    fn test_do_unquote_unterminated() {
        assert_eq!(do_unquote(r#""abc"#), Err(ParsingError::ParseQuotedString));
        // escape encountered but string never closes
        assert_eq!(
            do_unquote(r#""abc\"#),
            Err(ParsingError::ParseQuotedString)
        );
    }

    #[test]
    fn test_do_unquote_empty_body() {
        assert_eq!(do_unquote(r#""""#), Ok(("", Cow::Borrowed(""))));
    }

    #[test]
    fn test_do_unquote_fast_path_is_borrowed() {
        // no escapes at all -> must be the zero-copy Cow::Borrowed path
        let (remaining, s) = do_unquote(r#""hello world"tail"#).unwrap();
        assert_eq!(remaining, "tail");
        assert!(matches!(s, Cow::Borrowed("hello world")));
    }

    #[test]
    fn test_do_unquote_slow_path_is_owned() {
        // an escape forces the owned/allocating path
        let (remaining, s) = do_unquote(r#""a\"b"tail"#).unwrap();
        assert_eq!(remaining, "tail");
        assert!(matches!(s, Cow::Owned(_)));
        assert_eq!(s, "a\"b");
    }

    #[test]
    fn test_do_unquote_escaped_backslash() {
        assert_eq!(
            do_unquote(r#""a\\b"rest"#),
            Ok(("rest", Cow::Owned::<str>("a\\b".to_string())))
        );
    }

    #[test]
    fn test_do_unquote_leading_escape() {
        // escape sequence right at the very start of the body
        assert_eq!(
            do_unquote(r#""\"quoted\""rest"#),
            Ok(("rest", Cow::Owned::<str>("\"quoted\"".to_string())))
        );
    }

    #[test]
    fn test_unquote_error_paths() {
        assert_eq!(
            Unquote.parse("no quotes"),
            Err(ParsingError::ParseQuotedString)
        );
        assert_eq!(
            Unquote.parse(r#""unterminated"#),
            Err(ParsingError::ParseQuotedString)
        );
        assert_eq!(Unquote.parse(""), Err(ParsingError::ParseQuotedString));
    }

    // ----- user_id / asset_identifier leaf parsers -----

    #[test]
    fn test_user_id_parser() {
        assert_eq!(
            user_id().parse(r#""Steeve"rest"#),
            Ok(("rest", "Steeve".parse().unwrap()))
        );
        // empty user id string is rejected by UserId::from_str
        assert_eq!(
            user_id().parse(r#"""rest"#),
            Err(ParsingError::ParseUserIdError)
        );
        assert_eq!(
            user_id().parse("no quotes"),
            Err(ParsingError::ParseQuotedString)
        );
    }

    #[test]
    fn test_asset_identifier_parser() {
        assert_eq!(
            asset_identifier().parse(r#""bayc"rest"#),
            Ok(("rest", "bayc".parse().unwrap()))
        );
        assert_eq!(
            asset_identifier().parse(r#"""rest"#),
            Err(ParsingError::ParseUserIdError)
        );
    }

    // ----- AsIs -----

    #[test]
    fn test_as_is() {
        assert_eq!(AsIs.parse("everything"), Ok(("", "everything")));
        assert_eq!(AsIs.parse(""), Ok(("", "")));
    }

    // ----- Tag / QuotedTag error paths -----

    #[test]
    fn test_tag_empty_input() {
        assert_eq!(tag("key").parse(""), Err(ParsingError::ParseTagError));
    }

    #[test]
    fn test_quoted_tag_error_paths() {
        assert_eq!(
            quoted_tag("key").parse(r#""other"=value"#),
            Err(ParsingError::ParseTagError)
        );
        assert_eq!(
            quoted_tag("key").parse(""),
            Err(ParsingError::ParseQuotedString)
        );
        // trailing junk inside the quotes after the tag itself must fail
        assert_eq!(
            quoted_tag("key").parse(r#""keyextra"=value"#),
            Err(ParsingError::ParseTagError)
        );
    }

    // ----- StripWhitespace error propagation -----

    #[test]
    fn test_strip_whitespace_error_propagates() {
        assert_eq!(
            strip_whitespace(tag("hello")).parse("   goodbye"),
            Err(ParsingError::ParseTagError)
        );
    }

    // ----- Preceded -----

    #[test]
    fn test_preceded() {
        assert_eq!(
            preceded(tag("key="), NonZeroU32::MIN).parse("key=42rest"),
            Ok(("rest", NonZeroU32::new(42).unwrap()))
        );
        assert_eq!(
            preceded(tag("key="), NonZeroU32::MIN).parse("nope=42"),
            Err(ParsingError::ParseTagError)
        );
        // prefix matches but the inner parser fails
        assert_eq!(
            preceded(tag("key="), NonZeroU32::MIN).parse("key=nope"),
            Err(ParsingError::ParseIntError("".parse::<u32>().unwrap_err()))
        );
    }

    // ----- All (3 and 4 args) -----

    #[test]
    fn test_all3() {
        assert_eq!(
            all3(tag("a"), NonZeroU32::MIN, tag("z")).parse("a42zrest"),
            Ok(("rest", ((), NonZeroU32::new(42).unwrap(), ())))
        );
        assert!(
            all3(tag("a"), NonZeroU32::MIN, tag("z"))
                .parse("a42y")
                .is_err()
        );
    }

    #[test]
    fn test_all4() {
        assert_eq!(
            all4(tag("a"), NonZeroU32::MIN, tag("z"), NonZeroU32::MIN)
                .parse("a42z7rest"),
            Ok((
                "rest",
                (
                    (),
                    NonZeroU32::new(42).unwrap(),
                    (),
                    NonZeroU32::new(7).unwrap()
                )
            ))
        );
    }

    // ----- Permutation (3 args, all orderings) -----

    #[test]
    fn test_permutation3_all_orderings() {
        let expected = (
            NonZeroU32::new(1).unwrap(),
            NonZeroU32::new(2).unwrap(),
            NonZeroU32::new(3).unwrap(),
        );
        for input in [
            r#""a":1,"b":2,"c":3,"#,
            r#""a":1,"c":3,"b":2,"#,
            r#""b":2,"a":1,"c":3,"#,
            r#""b":2,"c":3,"a":1,"#,
            r#""c":3,"a":1,"b":2,"#,
            r#""c":3,"b":2,"a":1,"#,
        ] {
            assert_eq!(
                permutation3(
                    key_value("a", NonZeroU32::MIN),
                    key_value("b", NonZeroU32::MIN),
                    key_value("c", NonZeroU32::MIN),
                )
                .parse(input),
                Ok(("", expected)),
                "failed for input {input:?}"
            );
        }
    }

    #[test]
    fn test_permutation3_missing_member_fails() {
        assert!(
            permutation3(
                key_value("a", NonZeroU32::MIN),
                key_value("b", NonZeroU32::MIN),
                key_value("c", NonZeroU32::MIN),
            )
            .parse(r#""a":1,"b":2,"#)
            .is_err()
        );
    }

    // ----- List edge cases -----

    #[test]
    fn test_list_missing_open_bracket() {
        assert_eq!(
            list(NonZeroU32::MIN).parse("1,2,]"),
            Err(ParsingError::ParseListError)
        );
    }

    #[test]
    fn test_list_missing_trailing_comma() {
        assert_eq!(
            list(NonZeroU32::MIN).parse("[1,2]"),
            Err(ParsingError::ParseListError)
        );
    }

    #[test]
    fn test_list_unterminated() {
        assert_eq!(
            list(NonZeroU32::MIN).parse("[1,2,"),
            Err(ParsingError::ParseListError)
        );
    }

    #[test]
    fn test_list_of_lists() {
        assert_eq!(
            list(list(NonZeroU32::MIN)).parse("[[1,2,],[3,],[],]"),
            Ok((
                "",
                vec![
                    vec![
                        NonZeroU32::new(1).unwrap(),
                        NonZeroU32::new(2).unwrap()
                    ],
                    vec![NonZeroU32::new(3).unwrap()],
                    vec![],
                ]
            ))
        );
    }

    // ----- Alt (3, 4, 8 args) -----

    #[test]
    fn test_alt3() {
        let p = alt3(tag("a"), tag("b"), tag("c"));
        assert_eq!(p.parse("arest"), Ok(("rest", ())));
        assert_eq!(p.parse("brest"), Ok(("rest", ())));
        assert_eq!(p.parse("crest"), Ok(("rest", ())));
        assert_eq!(p.parse("drest"), Err(ParsingError::ParseTagError));
    }

    #[test]
    fn test_alt4() {
        let p = alt4(tag("a"), tag("b"), tag("c"), tag("d"));
        assert_eq!(p.parse("drest"), Ok(("rest", ())));
        assert_eq!(p.parse("erest"), Err(ParsingError::ParseTagError));
    }

    #[test]
    fn test_alt8_picks_first_match_and_last_error() {
        let p = alt8(
            tag("a"),
            tag("b"),
            tag("c"),
            tag("d"),
            tag("e"),
            tag("f"),
            tag("g"),
            tag("h"),
        );
        for (letter, rest) in [
            ("a", "1"),
            ("b", "1"),
            ("c", "1"),
            ("d", "1"),
            ("e", "1"),
            ("f", "1"),
            ("g", "1"),
            ("h", "1"),
        ] {
            assert_eq!(p.parse(&format!("{letter}{rest}")), Ok((rest, ())));
        }
        assert_eq!(p.parse("z1"), Err(ParsingError::ParseTagError));
    }

    // ----- Take -----

    #[test]
    fn test_take() {
        assert_eq!(
            take(3, stdp::Byte).parse("0a1b2crest"),
            Ok(("rest", vec![0x0a, 0x1b, 0x2c]))
        );
        assert_eq!(take(0, stdp::Byte).parse("rest"), Ok(("rest", vec![])));
        // not enough input for the requested count
        assert!(take(3, stdp::Byte).parse("0a1b").is_err());
    }

    // ----- Status -----

    #[test]
    fn test_status() {
        assert_eq!(Status::parser().parse("Ok"), Ok(("", Status::Ok)));
        assert_eq!(Status::parser().parse("Okrest"), Ok(("rest", Status::Ok)));
        match Status::parser().parse(r#"Err("oops")rest"#) {
            Ok(("rest", Status::Err(msg))) => assert_eq!(msg, "oops"),
            other => panic!("unexpected result: {other:?}"),
        }
        assert!(Status::parser().parse("Neither").is_err());
    }

    // ----- UserCash / UserBucket / UserBuckets / Announcements (direct) -----

    #[test]
    fn test_user_cash_parser_direct() {
        assert_eq!(
            UserCash::parser()
                .parse(r#"UserCash{"user_id":"Steeve","count":10,}"#),
            Ok((
                "",
                UserCash::new(
                    "Steeve".parse().unwrap(),
                    NonZeroU32::new(10).unwrap()
                )
            ))
        );
    }

    #[test]
    fn test_user_bucket_parser_direct() {
        assert_eq!(
            UserBucket::parser().parse(
                r#"UserBucket{"user_id":"Steeve","Bucket":Bucket{"asset_id":"bayc","count":1,},}"#
            ),
            Ok((
                "",
                UserBucket::new(
                    "Steeve".parse().unwrap(),
                    Bucket::new("bayc".parse().unwrap(), 1)
                )
            ))
        );
    }

    #[test]
    fn test_user_buckets_parser() {
        assert_eq!(
            UserBuckets::parser().parse(
                r#"UserBuckets{"user_id":"Steeve","buckets":[Bucket{"asset_id":"bayc","count":1,},Bucket{"asset_id":"usd","count":5,},],}"#
            ),
            Ok((
                "",
                UserBuckets::new(
                    "Steeve".parse().unwrap(),
                    vec![
                        Bucket::new("bayc".parse().unwrap(), 1),
                        Bucket::new("usd".parse().unwrap(), 5),
                    ]
                )
            ))
        );
    }

    #[test]
    fn test_announcements_parser() {
        let (remaining, announcements) = Announcements::parser()
            .parse(
                r#"[UserBuckets{"user_id":"Steeve","buckets":[Bucket{"asset_id":"bayc","count":1,},],},]"#
            )
            .unwrap();
        assert_eq!(remaining, "");
        assert_eq!(
            announcements,
            Announcements::new(vec![UserBuckets::new(
                "Steeve".parse().unwrap(),
                vec![Bucket::new("bayc".parse().unwrap(), 1)]
            )])
        );
        assert_eq!(
            Announcements::parser().parse("[]"),
            Ok(("", Announcements::new(vec![])))
        );
    }

    // ----- SystemLogTraceKind / AppLogErrorKind / AppLogTraceKind (remaining branches) -----

    #[test]
    fn test_system_log_trace_kind() {
        assert_eq!(
            SystemLogTraceKind::parser().parse(r#"Trace SendRequest "ping""#),
            Ok(("", SystemLogTraceKind::SendRequest("ping".into())))
        );
        assert_eq!(
            SystemLogTraceKind::parser().parse(r#"Trace GetResponse "pong""#),
            Ok(("", SystemLogTraceKind::GetResponse("pong".into())))
        );
    }

    #[test]
    fn test_system_log_kind_trace_branch() {
        assert_eq!(
            LogKind::parser().parse(r#"System::Trace SendRequest "ping""#),
            Ok((
                "",
                LogKind::System(SystemLogKind::Trace(
                    SystemLogTraceKind::SendRequest("ping".into())
                ))
            ))
        );
    }

    #[test]
    fn test_app_log_error_kind() {
        assert_eq!(
            AppLogErrorKind::parser().parse(r#"Error LackOf "gas""#),
            Ok(("", AppLogErrorKind::LackOf("gas".into())))
        );
        assert_eq!(
            AppLogErrorKind::parser().parse(r#"Error SystemError "disk full""#),
            Ok(("", AppLogErrorKind::SystemError("disk full".into())))
        );
    }

    #[test]
    fn test_app_log_kind_error_branch() {
        assert_eq!(
            LogKind::parser().parse(r#"App::Error LackOf "gas""#),
            Ok((
                "",
                LogKind::App(AppLogKind::Error(AppLogErrorKind::LackOf(
                    "gas".into()
                )))
            ))
        );
    }

    #[test]
    fn test_app_log_trace_kind_get_response() {
        assert_eq!(
            AppLogTraceKind::parser().parse(r#"Trace GetResponse "ok""#),
            Ok(("", AppLogTraceKind::GetResponse("ok".into())))
        );
    }

    #[test]
    fn test_app_log_trace_kind_check_announcements() {
        let (remaining, trace) = AppLogTraceKind::parser()
            .parse(
                r#"Trace Check [UserBuckets{"user_id":"Steeve","buckets":[Bucket{"asset_id":"bayc","count":1,},],},]"#
            )
            .unwrap();
        assert_eq!(remaining, "");
        match trace {
            AppLogTraceKind::Check(_) => {}
            other => panic!("expected Check variant, got {other:?}"),
        }
    }

    // ----- AppLogJournalKind: remaining variants -----

    #[test]
    fn test_journal_register_asset() {
        assert_eq!(
            AppLogJournalKind::parser().parse(
                r#"Journal RegisterAsset {"asset_id":"bayc","user_id":"Steeve","liquidity":100,}"#
            ),
            Ok((
                "",
                AppLogJournalKind::RegisterAsset {
                    asset_id: "bayc".parse().unwrap(),
                    user_id: "Steeve".parse().unwrap(),
                    liquidity: NonZeroU32::new(100).unwrap(),
                }
            ))
        );
    }

    #[test]
    fn test_journal_unregister_asset() {
        assert_eq!(
            AppLogJournalKind::parser().parse(
                r#"Journal UnregisterAsset {"asset_id":"bayc","user_id":"Steeve",}"#
            ),
            Ok((
                "",
                AppLogJournalKind::UnregisterAsset {
                    asset_id: "bayc".parse().unwrap(),
                    user_id: "Steeve".parse().unwrap(),
                }
            ))
        );
    }

    #[test]
    fn test_journal_withdraw_cash() {
        // regression test: this used to be mismapped to DepositCash
        assert_eq!(
            AppLogJournalKind::parser().parse(
                r#"Journal WithdrawCash UserCash{"user_id":"Steeve","count":10,}"#
            ),
            Ok((
                "",
                AppLogJournalKind::WithdrawCash(UserCash::new(
                    "Steeve".parse().unwrap(),
                    NonZeroU32::new(10).unwrap()
                ))
            ))
        );
    }

    #[test]
    fn test_journal_deposit_cash_not_mixed_up_with_withdraw() {
        assert_eq!(
            AppLogJournalKind::parser().parse(
                r#"Journal DepositCash UserCash{"user_id":"Steeve","count":10,}"#
            ),
            Ok((
                "",
                AppLogJournalKind::DepositCash(UserCash::new(
                    "Steeve".parse().unwrap(),
                    NonZeroU32::new(10).unwrap()
                ))
            ))
        );
    }

    #[test]
    fn test_journal_sell_asset() {
        assert_eq!(
            AppLogJournalKind::parser().parse(
                r#"Journal SellAsset UserBucket{"user_id":"Steeve","Bucket":Bucket{"asset_id":"bayc","count":1,},}"#
            ),
            Ok((
                "",
                AppLogJournalKind::SellAsset(UserBucket::new(
                    "Steeve".parse().unwrap(),
                    Bucket::new("bayc".parse().unwrap(), 1)
                ))
            ))
        );
    }

    // ----- LogLine / LOG_LINE_PARSER: full end-to-end lines -----

    #[test]
    fn test_log_line_full_parse() {
        let (remaining, line) = LogLine::parser()
            .parse(r#"System::Error NetworkError "url unknown" requestid=42"#)
            .unwrap();
        assert_eq!(remaining, "");
        assert_eq!(line.request_id(), 42);
        assert!(line.is_error());
        assert!(!line.is_exchange());
    }

    #[test]
    fn test_log_line_parser_static() {
        let (remaining, line) = LOG_LINE_PARSER
            .parse(
                r#"App::Journal DeleteUser {"user_id": "Steeve",} requestid=7"#,
            )
            .unwrap();
        assert_eq!(remaining, "");
        assert_eq!(line.request_id(), 7);
        assert!(!line.is_exchange());
    }

    #[test]
    fn test_log_line_is_exchange_flags() {
        let (_, line) = LOG_LINE_PARSER
            .parse(r#"App::Journal BuyAsset UserBucket{"user_id": "Steeve", "Bucket": Bucket{"asset_id":"bayc","count":1,},} requestid=1"#)
            .unwrap();
        assert!(line.is_exchange());
        assert!(!line.is_error());
    }

    #[test]
    fn test_log_line_missing_request_id_fails() {
        assert!(
            LOG_LINE_PARSER
                .parse(r#"System::Error NetworkError "url unknown""#)
                .is_err()
        );
    }

    #[test]
    fn test_log_line_trailing_garbage_is_reported_as_remaining() {
        // the parser itself doesn't require full consumption - callers
        // (like LogIterator) are responsible for checking `remaining`.
        // note: the trailing StripWhitespace around the request-id parser
        // also eats the leading space of whatever comes after it.
        let (remaining, _) = LOG_LINE_PARSER
            .parse(r#"App::Journal DeleteUser {"user_id": "Steeve",} requestid=7 trailing junk"#)
            .unwrap();
        assert_eq!(remaining, "trailing junk");
    }

    // ----- stdp::Byte error paths -----

    #[test]
    fn test_byte_parser_errors() {
        assert_eq!(stdp::Byte.parse("a"), Err(ParsingError::SplitStringError));
        assert!(stdp::Byte.parse("zz").is_err());
        assert_eq!(stdp::Byte.parse("ff"), Ok(("", 0xff)));
    }
}
