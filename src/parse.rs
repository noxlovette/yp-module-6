/// Трейт, чтобы **реализовывать** и **требовать** метод 'распарсь и покажи,
/// что распарсить осталось'
trait Parser {
    type Dest;
    // подсказка: здесь можно переделать
    // на `fn parse<'a>(&self,input:&'a str)->Result<(&'a str, Self::Dest)>`
    // (возможно, самое трудоёмкое; в своих проектах проще сразу не допускать)
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()>;
}
/// Вспомогательный трейт, чтобы писать собственный десериализатор
/// (по решаемой задаче - отдалённый аналог `serde::Deserialize`)
trait Parsable : Sized {
    type Parser: Parser<Dest=Self>;
    fn parser () -> Self::Parser;
}

mod stdp { // parsers for std types
    use super::Parser;

    /// Беззнаковые числа
    #[derive(Debug)]
    pub struct U32;
    impl Parser for U32 {
        type Dest = u32;
        fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
            let (remaining, is_hex) = input.strip_prefix("0x")
                                           .map_or((input.to_string(), false), |remaining| (remaining.to_string(), true));
            let end_idx = remaining.char_indices().find_map(
                |(idx, c)| match (is_hex, c) {
                    (true, 'a'..='f'|'0'..='9'|'A'..='F') => None,
                    (false, '0'..='9') => None,
                    _ => Some(idx)
                }
            ).unwrap_or(remaining.len());
            let value = u32::from_str_radix(
                    &remaining[..end_idx],
                    if is_hex {16} else {10}
                ).map_err(|_| ())?;
            // подсказка: вместо if можно использовать tight-тип std::num::NonZeroU32
            //            (ограничиться NonZeroU32::new(value).ok_or(()).get() - норм)
            //            или даже заиспользовать tightness
            if value == 0 {
                return Err(()); // в наших логах нет нулей, ноль в операции - фикция
            }
            Ok((
                remaining[end_idx..].to_string(),
                value
            ))
        }
    }
    /// Знаковые числа
    #[derive(Debug)]
    pub struct I32;
    impl Parser for I32 {
        type Dest = i32;
        fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
            let end_idx = input.char_indices().skip(1)
                               .find_map(|(idx, c)| (!c.is_ascii_digit()).then_some(idx))
                               .unwrap_or(input.len());
            let value = input[..end_idx].parse().map_err(|_| ())?;
            if value == 0 {
                return Err(()); // в наших логах нет нулей, ноль в операции - фикция
            }
            Ok((input[end_idx..].to_string(), value))
        }
    }
    /// Шестнадцатеричные байты (пригодится при парсинге блобов)
    #[derive(Debug, Clone)]
    pub struct Byte;
    impl Parser for Byte {
        type Dest = u8;
        fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
            let (to_parse, remaining) = input.split_at_checked(2).ok_or(())?;
            if !to_parse.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(());
            }
            let value = u8::from_str_radix(to_parse, 16).map_err(|_| ())?;
            Ok((remaining.to_string(), value))
        }
    }
}

/// Обернуть строку в кавычки, экранировав кавычки, которые в строке уже есть
fn quote (input: &str) -> String {
    let mut result = String::from("\"");
    result.extend(input.chars()
        .map(|c| match c {
            '\\' | '"' => ['\\', c].into_iter().take(2),
            _ => [c, ' '].into_iter().take(1)
        })
        .flatten()
    );
    result.push('"');
    result
}
/// Распарсить строку, которую ранее [обернули в кавычки](quote)
// `"abc\"def\\ghi"nice` -> (`abcd"def\ghi`, `nice`)
fn do_unquote (input: String) -> Result<(String, String), ()> {
    let mut result = String::new();
    let mut escaped_now = false;
    let mut chars = input.strip_prefix("\"").ok_or(())?.chars();
    while let Some(c) = chars.next() {
        match (c, escaped_now) {
            ('"' | '\\', true) => {
                result.push(c);
                escaped_now = false;
            },
            ('\\', false) => escaped_now = true,
            ('"', false) => return Ok((chars.as_str().to_string(), result)),
            (c, _) => {
                result.push(c);
                escaped_now = false;
            },
        }
    }
    Err(()) // строка кончилась, не закрыв кавычку
}
/// Распарсить строку, обёрную в кавычки
/// (сокращённая версия [do_unquote], в которой вложенные кавычки не предусмотрены)
fn do_unquote_non_escaped (input: String) -> Result<(String, String), ()> {
    let input = input.strip_prefix("\"").ok_or(())?;
    let quote_byteidx = input.find('"').ok_or(())?;
    if 0 == quote_byteidx || Some("\\") == input.get(quote_byteidx - 1..quote_byteidx) {
        return Err(());
    }
    Ok((input[1+quote_byteidx..].to_string(), input[..quote_byteidx].to_string()))
}
/// Парсер кавычек
#[derive(Debug, Clone)]
struct Unquote;
impl Parser for Unquote {
    type Dest = String;
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        do_unquote(input)
    }
}
/// Конструктор [Unquote]
fn unquote() -> Unquote {
    Unquote
}
/// Парсер, возвращающий результат как есть
#[derive(Debug, Clone)]
struct AsIs;
impl Parser for AsIs {
    type Dest = String;
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        Ok((input[input.len()..].to_string(), input.into()))
    }
}
/// Парсер константных строк
/// (аналог `nom::bytes::complete::tag`)
#[derive(Debug, Clone)]
struct Tag {
    tag: &'static str,
}
impl Parser for Tag {
    type Dest = ();
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        Ok((input.strip_prefix(self.tag).ok_or(())?.to_string(), ()))
    }
}
/// Конструктор [Tag]
fn tag(tag: &'static str) -> Tag {
    Tag{tag}
}
/// Парсер [тэга](Tag), обёрнутого в кавычки
#[derive(Debug, Clone)]
struct QuotedTag(Tag);
impl Parser for QuotedTag {
    type Dest = ();
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        let (remaining, candidate) = do_unquote_non_escaped(input)?;
        if !self.0.parse(candidate)?.0.is_empty() {
            return  Err(());
        }
        Ok((remaining, ()))
    }
}
/// Конструктор [QuotedTag]
fn quoted_tag(tag: &'static str) -> QuotedTag {
    QuotedTag(Tag{tag})
}
/// Комбинатор, пробрасывающий строку без лидирующих пробелов
#[derive(Debug, Clone)]
struct StripWhitespace<T> {
    parser: T,
}
impl<T: Parser> Parser for StripWhitespace<T> {
    type Dest = T::Dest;
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        self.parser.parse(input.trim_start().to_string()).map(
            |(remaining, parsed)| (remaining.trim_start().to_string(), parsed)
        )
    }
}
/// Конструктор [StripWhitespace]
fn strip_whitespace<T: Parser>(parser: T) -> StripWhitespace<T> {
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
struct Delimited<Prefix, T, Suffix> {
    prefix_to_ignore: Prefix,
    dest_parser: T,
    suffix_to_ignore: Suffix
}
impl<Prefix, T, Suffix> Parser for Delimited<Prefix, T, Suffix>
where Prefix: Parser,
      T: Parser,
      Suffix: Parser,
{
    type Dest = T::Dest;
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        let (remaining, _) = self.prefix_to_ignore.parse(input)?;
        let (remaining, result) = self.dest_parser.parse(remaining)?;
        self.suffix_to_ignore.parse(remaining).map(|(remaining, _)| (remaining, result))
    }
}
/// Конструктор [Delimited]
fn delimited<Prefix, T, Suffix>(
    prefix_to_ignore: Prefix, dest_parser: T, suffix_to_ignore: Suffix
) -> Delimited<Prefix, T, Suffix>
where Prefix: Parser,
      T: Parser,
      Suffix: Parser,
{
    Delimited { prefix_to_ignore, dest_parser, suffix_to_ignore }
}
/// Комбинатор-отображение. Парсит дочерним парсером, преобразует результат так,
/// как вызывающему хочется
#[derive(Debug, Clone)]
struct Map<T,M> {
    parser: T,
    map: M,
}
impl<T: Parser, Dest: Sized, M: Fn(T::Dest)->Dest> Parser for Map<T, M> {
    type Dest = Dest;
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        self.parser.parse(input).map(|(remaining, pre_result)| (remaining, (self.map)(pre_result)))
    }
}
/// Конструктор [Map]
fn map<T: Parser, Dest: Sized, M: Fn(T::Dest)->Dest>(parser: T, map: M) -> Map<T, M> {
    Map{parser, map}
}
/// Комбинатор с отбрасываемым префиксом, упрощённая версия [Delimited]
/// (аналог `preceeded` из `nom`)
#[derive(Debug, Clone)]
struct Preceded<Prefix, T> {
    prefix_to_ignore: Prefix,
    dest_parser: T,
}
impl<Prefix, T> Parser for Preceded<Prefix, T>
where Prefix: Parser,
      T: Parser
{
    type Dest = T::Dest;
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        let (remaining, _) = self.prefix_to_ignore.parse(input)?;
        self.dest_parser.parse(remaining)
    }
}
/// Конструктор [Preceded]
fn preceded<Prefix, T>(prefix_to_ignore: Prefix, dest_parser: T) -> Preceded<Prefix, T>
where Prefix: Parser,
      T: Parser
{
    Preceded { prefix_to_ignore, dest_parser }
}
/// Комбинатор, который требует, чтобы все дочерние парсеры отработали,
/// (аналог `all` из `nom`)
#[derive(Debug, Clone)]
struct All<T> {
    parser: T,
}
impl<A0,A1> Parser for All<(A0,A1)>
where A0: Parser,
      A1: Parser,
{
    type Dest = (A0::Dest, A1::Dest);
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        let (remaining, a0) = self.parser.0.parse(input)?;
        self.parser.1.parse(remaining).map(
            |(remaining, a1)| (remaining, (a0, a1))
        )
    }
}
/// Конструктор [All] для двух парсеров
/// (в Rust нет чего-то, вроде variadic templates из C++)
fn all2<A0: Parser, A1: Parser>(a0: A0, a1: A1) -> All<(A0,A1)> {
    All { parser: (a0, a1) }
}
impl<A0,A1,A2> Parser for All<(A0,A1,A2)>
where A0: Parser,
      A1: Parser,
      A2: Parser,
{
    type Dest = (A0::Dest, A1::Dest, A2::Dest);
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        let (remaining, a0) = self.parser.0.parse(input)?;
        let (remaining, a1) = self.parser.1.parse(remaining)?;
        self.parser.2.parse(remaining).map(
            |(remaining, a2)| (remaining, (a0, a1, a2))
        )
    }
}
/// Конструктор [All] для трёх парсеров
/// (в Rust нет чего-то, вроде variadic templates из C++)
fn all3<A0: Parser, A1: Parser, A2: Parser>(a0: A0, a1: A1, a2: A2) -> All<(A0,A1,A2)> {
    All { parser: (a0, a1, a2) }
}
impl<A0,A1,A2,A3> Parser for All<(A0,A1,A2,A3)>
where A0: Parser,
      A1: Parser,
      A2: Parser,
      A3: Parser,
{
    type Dest = (A0::Dest, A1::Dest, A2::Dest, A3::Dest);
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        let (remaining, a0) = self.parser.0.parse(input)?;
        let (remaining, a1) = self.parser.1.parse(remaining)?;
        let (remaining, a2) = self.parser.2.parse(remaining)?;
        self.parser.3.parse(remaining).map(
            |(remaining, a3)| (remaining, (a0, a1, a2, a3))
        )
    }
}
/// Конструктор [All] для четырёх парсеров
/// (в Rust нет чего-то, вроде variadic templates из C++)
fn all4<A0: Parser, A1: Parser, A2: Parser, A3: Parser>(a0: A0, a1: A1, a2: A2, a3: A3) -> All<(A0,A1,A2,A3)> {
    All { parser: (a0, a1, a2, a3) }
}
/// Комбинатор, который вытаскивает значения из пары `"ключ":значение,`.
/// Для простоты реализации, запятая всегда нужна в конце пары ключ-значение,
/// простое '"ключ":значение' читаться не будет
#[derive(Debug, Clone)]
struct KeyValue<T> {
    parser: Delimited<All<(StripWhitespace<QuotedTag>,StripWhitespace<Tag>)>,StripWhitespace<T>,StripWhitespace<Tag>>
}
impl<T> Parser for KeyValue<T>
where T: Parser {
    type Dest = T::Dest;
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        self.parser.parse(input)
    }
}
/// Конструктор [KeyValue]
fn key_value<T: Parser>(key: &'static str, value_parser: T) -> KeyValue<T> {
    KeyValue { parser: 
        delimited(
            all2(
                strip_whitespace(quoted_tag(key)),
                strip_whitespace(tag(":"))
            ),
            strip_whitespace(value_parser),
            strip_whitespace(tag(","))
        )
    }
}
/// Комбинатор, который возвращает результаты дочерних парсеров, если их
/// удалось применить друг после друга в любом порядке. Результат возвращается в
/// том порядке, в каком `Permutation` был сконструирован
/// (аналог `permutation` из `nom`)
#[derive(Debug, Clone)]
struct Permutation<T> {
    parsers: T,
}
impl<A0,A1> Parser for Permutation<(A0,A1)>
where A0: Parser,
      A1: Parser,
{
    type Dest = (A0::Dest, A1::Dest);
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        match self.parsers.0.parse(input.clone()) {
            Ok((remaining, a0))
                => self.parsers.1.parse(remaining).map(
                    |(remaining, a1)| (remaining, (a0, a1))
                ),
            Err(())
                => self.parsers.1.parse(input).and_then(
                    |(remaining, a1)| self.parsers.0.parse(remaining).map(
                        |(remaining, a0)| (remaining, (a0, a1))
                    )
                ),
        }
    }
}
/// Конструктор [Permutation] для двух парсеров
/// (в Rust нет чего-то, вроде variadic templates из C++)
fn permutation2<A0: Parser, A1: Parser>(a0: A0, a1: A1) -> Permutation<(A0, A1)> {
    Permutation { parsers: (a0, a1) }
}
impl<A0,A1,A2> Parser for Permutation<(A0,A1,A2)>
where A0: Parser,
      A1: Parser,
      A2: Parser,
{
    type Dest = (A0::Dest, A1::Dest, A2::Dest);
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        match self.parsers.0.parse(input.clone()) {
            Ok((remaining, a0))
                => match self.parsers.1.parse(remaining.clone()) {
                    Ok((remaining, a1)) => self.parsers.2.parse(remaining).map(
                        |(remaining, a2)| (remaining, (a0, a1, a2))
                    ),
                    Err(()) => self.parsers.2.parse(remaining.clone()).and_then(
                        |(remaining, a2)| self.parsers.1.parse(remaining).map(
                            |(remaining, a1)| (remaining, (a0, a1, a2))
                        )
                    )
                }
            Err(())
                => match self.parsers.1.parse(input.clone()) {
                    Ok((remaining, a1)) => match self.parsers.0.parse(remaining.clone()) {
                        Ok((remaining, a0)) => self.parsers.2.parse(remaining).map(
                            |(remaining, a2)| (remaining, (a0, a1, a2))
                        ),
                        Err(()) => self.parsers.2.parse(remaining.clone()).and_then(
                            |(remaining, a2)| self.parsers.0.parse(remaining).map(
                                |(remaining, a0)| (remaining, (a0, a1, a2))
                            )
                        )
                    },
                    Err(()) => self.parsers.2.parse(input.clone()).and_then(
                        |(remaining, a2)| match self.parsers.0.parse(remaining.clone()) {
                            Ok((remaining, a0)) => self.parsers.1.parse(remaining).map(
                                |(remaining, a1)| (remaining, (a0, a1, a2))
                            ),
                            Err(()) => self.parsers.1.parse(remaining.clone()).and_then(
                                |(remaining, a1)| self.parsers.0.parse(remaining).map(
                                    |(remaining, a0)| (remaining, (a0, a1, a2))
                                )
                            )
                        }
                    )
                }
        }
    }
}
/// Конструктор [Permutation] для трёх парсеров
/// (в Rust нет чего-то, вроде variadic templates из C++)
fn permutation3<A0: Parser, A1: Parser, A2: Parser>(a0: A0, a1: A1, a2: A2) -> Permutation<(A0, A1, A2)> {
    Permutation { parsers: (a0, a1, a2) }
}
/// Комбинатор списка из любого числа элементов, которые надо читать
/// вложенным парсером. Граница списка определяется квадратными (`[`&`]`)
/// скобками.
/// Для простоты реализации, после каждого элемента списка должна быть запятая
#[derive(Debug, Clone)]
struct List<T> {
    parser: T
}
impl<T: Parser> Parser for List<T> {
    type Dest = Vec<T::Dest>;
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        let mut remaining = input.trim_start().strip_prefix('[').ok_or(())?.trim_start().to_string();
        let mut result = Vec::new();
        while !remaining.is_empty() {
            match remaining.strip_prefix(']') {
                Some(remaining) => return Ok((remaining.trim_start().to_string(), result)),
                None => {
                    let (new_remaining, item) = self.parser.parse(remaining.to_string())?;
                    let new_remaining = new_remaining.trim_start().strip_prefix(',').ok_or(())?.trim_start().to_string();
                    result.push(item);
                    remaining = new_remaining;
                },
            }
        }
        Err(()) // строка кончилась, не закрыв скобку
    }
}
/// Конструктор для [List]
fn list<T: Parser>(parser: T) -> List<T> {
    List { parser }
}
/// Комбинатор, который вернёт тот результат, который будет успешно
/// получен первым из дочерних комбинаторов
/// (аналог `alt` из `nom`)
#[derive(Debug, Clone)]
struct Alt<T> {
    parser: T,
}
impl<A0,A1,Dest> Parser for Alt<(A0,A1)>
where A0: Parser<Dest=Dest>,
      A1: Parser<Dest=Dest>,
{
    type Dest = Dest;
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        if let Ok(ok) = self.parser.0.parse(input.clone()) {
            return Ok(ok);
        }
        self.parser.1.parse(input)
    }
}
/// Конструктор [Alt] для двух парсеров
/// (в Rust нет чего-то, вроде variadic templates из C++)
fn alt2<Dest,A0: Parser<Dest=Dest>,A1: Parser<Dest=Dest>>(a0: A0, a1: A1) -> Alt<(A0,A1)> {
    Alt{parser:(a0, a1)}
}
impl<A0,A1,A2,Dest> Parser for Alt<(A0,A1,A2)>
where A0: Parser<Dest=Dest>,
      A1: Parser<Dest=Dest>,
      A2: Parser<Dest=Dest>,
{
    type Dest = Dest;
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        // match вместо тут не подойдёт - нужно лениво
        if let Ok(ok) = self.parser.0.parse(input.clone()) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.1.parse(input.clone()) {
            return Ok(ok);
        }
        self.parser.2.parse(input.clone())
    }
}
/// Конструктор [Alt] для трёх парсеров
/// (в Rust нет чего-то, вроде variadic templates из C++)
fn alt3<Dest,A0: Parser<Dest=Dest>,A1: Parser<Dest=Dest>, A2: Parser<Dest=Dest>>(a0: A0, a1: A1, a2: A2) -> Alt<(A0,A1,A2)> {
    Alt{parser:(a0, a1, a2)}
}
impl<A0,A1,A2,A3,Dest> Parser for Alt<(A0,A1,A2,A3)>
where A0: Parser<Dest=Dest>,
      A1: Parser<Dest=Dest>,
      A2: Parser<Dest=Dest>,
      A3: Parser<Dest=Dest>,
{
    type Dest = Dest;
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        if let Ok(ok) = self.parser.0.parse(input.clone()) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.1.parse(input.clone()) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.2.parse(input.clone()) {
            return Ok(ok);
        }
        self.parser.3.parse(input.clone())
    }
}
/// Конструктор [Alt] для четырёх парсеров
/// (в Rust нет чего-то, вроде variadic templates из C++)
fn alt4<Dest,A0: Parser<Dest=Dest>,A1: Parser<Dest=Dest>, A2: Parser<Dest=Dest>, A3: Parser<Dest=Dest>>(a0: A0, a1: A1, a2: A2, a3: A3) -> Alt<(A0,A1,A2,A3)> {
    Alt{parser:(a0, a1, a2, a3)}
}
impl<A0,A1,A2,A3,A4,A5,A6,A7,Dest> Parser for Alt<(A0,A1,A2,A3,A4,A5,A6,A7)>
where A0: Parser<Dest=Dest>,
      A1: Parser<Dest=Dest>,
      A2: Parser<Dest=Dest>,
      A3: Parser<Dest=Dest>,
      A4: Parser<Dest=Dest>,
      A5: Parser<Dest=Dest>,
      A6: Parser<Dest=Dest>,
      A7: Parser<Dest=Dest>,
{
    type Dest = Dest;
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
        if let Ok(ok) = self.parser.0.parse(input.clone()) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.1.parse(input.clone()) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.2.parse(input.clone()) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.3.parse(input.clone()) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.4.parse(input.clone()) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.5.parse(input.clone()) {
            return Ok(ok);
        }
        if let Ok(ok) = self.parser.6.parse(input.clone()) {
            return Ok(ok);
        }
        self.parser.7.parse(input.clone())
    }
}
/// Конструктор [Alt] для восьми парсеров
/// (в Rust нет чего-то, вроде variadic templates из C++)
fn alt8<Dest,A0: Parser<Dest=Dest>,A1: Parser<Dest=Dest>, A2: Parser<Dest=Dest>, A3: Parser<Dest=Dest>,A4:Parser<Dest=Dest>,A5:Parser<Dest=Dest>,A6:Parser<Dest=Dest>,A7:Parser<Dest=Dest>>(a0: A0, a1: A1, a2: A2, a3: A3, a4: A4, a5: A5, a6: A6, a7: A7) -> Alt<(A0,A1,A2,A3,A4,A5,A6,A7)> {
    Alt{parser:(a0, a1, a2, a3, a4, a5, a6, a7)}
}

/// Комбинатор для применения дочернего парсера N раз
/// (аналог `take` из `nom`)
struct Take<T> {
    count: usize,
    parser: T,
}
impl<T: Parser> Parser for Take<T> {
    type Dest = Vec<T::Dest>;
    fn parse(&self, input: String) -> Result<(String, Self::Dest), ()> {
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
/// Конструктор `Take`
fn take<T: Parser>(count: usize, parser: T) -> Take<T> {
    Take { count, parser }
}

const AUTHDATA_SIZE: usize = 1024;

// подсказка: довольно много места на стэке
/// Данные для авторизации
#[derive(Debug,Clone,PartialEq)]
pub struct AuthData([u8;AUTHDATA_SIZE]);
impl Parsable for AuthData {
    type Parser = Map<Take<stdp::Byte>,fn(Vec<u8>)->Self>;
    fn parser () -> Self::Parser {
        map(take(AUTHDATA_SIZE, stdp::Byte), |authdata| AuthData(authdata.try_into().unwrap_or([0;AUTHDATA_SIZE])))
    }
}

/// Конструкция 'либо-либо'
enum Either<Left,Right> {
    Left(Left),
    Right(Right),
}

/// Статус, которые можно парсить
enum Status {
    Ok,
    Err(String),
}
impl Parsable for Status {
    type Parser = Alt<(Map<Tag,fn(())->Self>,Map<Delimited<Tag,Unquote,Tag>,fn(String)->Self>)>;
    fn parser () -> Self::Parser {
        fn to_ok(_: ()) -> Status {
            Status::Ok
        }
        fn to_err(error: String) -> Status {
            Status::Err(error)
        }
        alt2(map(tag("Ok"), to_ok),map(delimited(tag("Err("),unquote(),tag(")")), to_err))
    }
}

/// Пара 'сокращённое название предмета' - 'его описание'
#[derive(Debug, Clone, PartialEq)]
pub struct AssetDsc { // `dsc` aka `description`
    pub id: String,
    pub dsc: String,
}
impl Parsable for AssetDsc {
    type Parser = Map<
        Delimited<
            All<(StripWhitespace<Tag>,StripWhitespace<Tag>)>,
            Permutation<(KeyValue<Unquote>,KeyValue<Unquote>)>,
            StripWhitespace<Tag>
        >,
        fn((String,String))->Self
    >;
    fn parser () -> Self::Parser {
        // комбинаторы парсеров - это круто
        map(delimited(
            all2(strip_whitespace(tag("AssetDsc")),strip_whitespace(tag("{"))),
            permutation2(key_value("id", unquote()), key_value("dsc", unquote())),
            strip_whitespace(tag("}"))
        ), |(id, dsc)| AssetDsc { id, dsc })
    }
}
/// Сведение о предмете в некотором количестве
#[derive(Debug, Clone, PartialEq)]
pub struct Backet {
    pub asset_id: String,
    pub count: u32,
}
impl Parsable for Backet {
    type Parser = Map<
        Delimited<
            All<(StripWhitespace<Tag>,StripWhitespace<Tag>)>,
            Permutation<(KeyValue<Unquote>,KeyValue<stdp::U32>)>,
            StripWhitespace<Tag>
        >,
        fn((String,u32))->Self
    >;
    fn parser () -> Self::Parser {
        map(delimited(
            all2(strip_whitespace(tag("Backet")),strip_whitespace(tag("{"))),
            permutation2(key_value("asset_id", unquote()), key_value("count", stdp::U32)),
            strip_whitespace(tag("}"))
        ), |(asset_id, count)| Backet { asset_id, count })
    }
}
/// Фиатные деньги конкретного пользователя
#[derive(Debug, Clone, PartialEq)]
pub struct UserCash {
    pub user_id: String,
    pub count: u32,
}
impl Parsable for UserCash {
    type Parser = Map<
        Delimited<
            All<(StripWhitespace<Tag>,StripWhitespace<Tag>)>,
            Permutation<(KeyValue<Unquote>,KeyValue<stdp::U32>)>,
            StripWhitespace<Tag>
        >,
        fn((String,u32))->Self
    >;
    fn parser () -> Self::Parser {
        map(delimited(
            all2(strip_whitespace(tag("UserCash")),strip_whitespace(tag("{"))),
            permutation2(key_value("user_id", unquote()), key_value("count", stdp::U32)),
            strip_whitespace(tag("}"))
        ), |(user_id, count)| UserCash { user_id, count })
    }
}
/// [Backet] конкретного пользователя
#[derive(Debug, Clone, PartialEq)]
pub struct UserBacket {
    pub user_id: String,
    pub backet: Backet,
}
impl Parsable for UserBacket {
    type Parser = Map<
        Delimited<
            All<(StripWhitespace<Tag>,StripWhitespace<Tag>)>,
            Permutation<(KeyValue<Unquote>,KeyValue<<Backet as Parsable>::Parser>)>,
            StripWhitespace<Tag>
        >,
        fn((String,Backet))->Self
    >;
    fn parser () -> Self::Parser {
        map(delimited(
            all2(strip_whitespace(tag("UserBacket")),strip_whitespace(tag("{"))),
            permutation2(key_value("user_id", unquote()), key_value("backet", Backet::parser())),
            strip_whitespace(tag("}"))
        ), |(user_id, backet)| UserBacket { user_id, backet })
    }
}
/// [Бакеты](Backet) конкретного пользователя
#[derive(Debug, Clone, PartialEq)]
pub struct UserBackets {
    pub user_id: String,
    pub backets: Vec<Backet>,
}
impl Parsable for UserBackets {
    type Parser = Map<
        Delimited<
            All<(StripWhitespace<Tag>,StripWhitespace<Tag>)>,
            Permutation<(KeyValue<Unquote>,KeyValue<List<<Backet as Parsable>::Parser>>)>,
            StripWhitespace<Tag>
        >,
        fn((String,Vec<Backet>))->Self
    >;
    fn parser () -> Self::Parser {
        map(delimited(
            all2(strip_whitespace(tag("UserBackets")),strip_whitespace(tag("{"))),
            permutation2(key_value("user_id", unquote()), key_value("backets", list(Backet::parser()))),
            strip_whitespace(tag("}"))
        ), |(user_id, backets)| UserBackets { user_id, backets })
    }
}
/// Список опубликованных бакетов
#[derive(Debug, Clone, PartialEq)]
pub struct Announcements(Vec<UserBackets>);
impl Parsable for Announcements {
    type Parser = Map<
        List<<UserBackets as Parsable>::Parser>,
        fn(Vec<UserBackets>)->Self
    >;
    fn parser () -> Self::Parser {
        fn from_vec(vec: Vec<UserBackets>) -> Announcements {
            Announcements(vec)
        }
        map(list(UserBackets::parser()), from_vec)
    }
}

// просто обёртки
// подсказка: почему бы не заменить на один дженерик?
/// Обёртка для парсинга [AssetDsc]
pub fn just_parse_asset_dsc(input: String) -> Result<(String, AssetDsc), ()> {
    <AssetDsc as Parsable>::parser().parse(input)
}
/// Обёртка для парсинга [Backet]
pub fn just_parse_backet(input: String) -> Result<(String, Backet), ()> {
    <Backet as Parsable>::parser().parse(input)
}
/// Обёртка для парсинга [UserCash]
pub fn just_user_cash(input: String) -> Result<(String, UserCash), ()> {
    <UserCash as Parsable>::parser().parse(input)
}
/// Обёртка для парсинга [UserBacket]
pub fn just_user_backet(input: String) -> Result<(String, UserBacket), ()> {
    <UserBacket as Parsable>::parser().parse(input)
}
/// Обёртка для парсинга [UserBackets]
pub fn just_user_backets(input: String) -> Result<(String, UserBackets), ()> {
    <UserBackets as Parsable>::parser().parse(input)
}
/// Обёртка для парсинга [Announcements]
pub fn just_parse_anouncements(input: String) -> Result<(String, Announcements), ()> {
    <Announcements as Parsable>::parser().parse(input)
}

/// Все виды логов
#[derive(Debug, Clone, PartialEq)]
pub enum LogKind {
    System(SystemLogKind),
    App(AppLogKind),
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
// подсказка: а поля не слишком много места на стэке занимают?
/// Trace [приложения](AppLogKind)
#[derive(Debug, Clone, PartialEq)]
pub enum AppLogTraceKind {
    Connect(AuthData),
    SendRequest(String),
    Check(Announcements),
    GetResponse(String),
}
/// Журнал [приложения](AppLogKind), самые высокоуровневые события
#[derive(Debug, Clone, PartialEq)]
pub enum AppLogJournalKind {
    CreateUser{user_id: String, authorized_capital: u32},
    DeleteUser{user_id: String},
    RegisterAsset{asset_id: String, user_id: String, liquidity: u32},
    UnregisterAsset{asset_id: String, user_id: String},
    DepositCash(UserCash),
    WithdrawCash(UserCash),
    BuyAsset(UserBacket),
    SellAsset(UserBacket),
}
impl Parsable for SystemLogErrorKind {
    type Parser = Preceded<Tag, Alt<(
        Map<Preceded<StripWhitespace<Tag>,StripWhitespace<Unquote>>,fn(String)->SystemLogErrorKind>,
        Map<Preceded<StripWhitespace<Tag>,StripWhitespace<Unquote>>,fn(String)->SystemLogErrorKind>
    )>>;
    fn parser () -> Self::Parser {
        preceded(
            tag("Error"),
            alt2(
                map(
                    preceded(
                        strip_whitespace(tag("NetworkError")),
                        strip_whitespace(unquote())
                    ),
                    |error| SystemLogErrorKind::NetworkError(error)
                ),
                map(
                    preceded(
                        strip_whitespace(tag("AccessDenied")),
                        strip_whitespace(unquote())
                    ),
                    |error| SystemLogErrorKind::AccessDenied(error)
                )
            )
        )
    }
}
impl Parsable for SystemLogTraceKind {
    type Parser = Preceded<Tag, Alt<(
        Map<Preceded<StripWhitespace<Tag>,StripWhitespace<Unquote>>,fn(String)->SystemLogTraceKind>,
        Map<Preceded<StripWhitespace<Tag>,StripWhitespace<Unquote>>,fn(String)->SystemLogTraceKind>
    )>>;
    fn parser () -> Self::Parser {
        preceded(
            tag("Trace"),
            alt2(
                map(
                    preceded(
                        strip_whitespace(tag("SendRequest")),
                        strip_whitespace(unquote())
                    ),
                    |request| SystemLogTraceKind::SendRequest(request)
                ),
                map(
                    preceded(
                        strip_whitespace(tag("GetResponse")),
                        strip_whitespace(unquote())
                    ),
                    |response| SystemLogTraceKind::GetResponse(response)
                )
            )
        )
    }
}
impl Parsable for SystemLogKind {
    type Parser = StripWhitespace<Preceded<Tag, Alt<(Map<<SystemLogTraceKind as Parsable>::Parser,fn(SystemLogTraceKind)->SystemLogKind>,Map<<SystemLogErrorKind as Parsable>::Parser,fn(SystemLogErrorKind)->SystemLogKind>)>>>;
    fn parser () -> Self::Parser {
        strip_whitespace(preceded(
            tag("System::"),
            alt2(
                map(SystemLogTraceKind::parser(), |trace| SystemLogKind::Trace(trace)),
                map(SystemLogErrorKind::parser(), |error| SystemLogKind::Error(error))
            )
        ))
    }
}
impl Parsable for AppLogErrorKind {
    type Parser = Preceded<Tag, Alt<(
        Map<Preceded<StripWhitespace<Tag>,StripWhitespace<Unquote>>,fn(String)->AppLogErrorKind>,
        Map<Preceded<StripWhitespace<Tag>,StripWhitespace<Unquote>>,fn(String)->AppLogErrorKind>
    )>>;
    fn parser () -> Self::Parser {
        preceded(
            tag("Error"),
            alt2(
                map(
                    preceded(
                        strip_whitespace(tag("LackOf")),
                        strip_whitespace(unquote())
                    ),
                    |error| AppLogErrorKind::LackOf(error)
                ),
                map(
                    preceded(
                        strip_whitespace(tag("SystemError")),
                        strip_whitespace(unquote())
                    ),
                    |error| AppLogErrorKind::SystemError(error)
                )
            )
        )
    }
}
impl Parsable for AppLogTraceKind {
    type Parser = Preceded<Tag, Alt<(
        Map<Preceded<StripWhitespace<Tag>,StripWhitespace<<AuthData as Parsable>::Parser>>,fn(AuthData)->AppLogTraceKind>,
        Map<Preceded<StripWhitespace<Tag>,StripWhitespace<Unquote>>,fn(String)->AppLogTraceKind>,
        Map<Preceded<StripWhitespace<Tag>,StripWhitespace<<Announcements as Parsable>::Parser>>,fn(Announcements)->AppLogTraceKind>,
        Map<Preceded<StripWhitespace<Tag>,StripWhitespace<Unquote>>,fn(String)->AppLogTraceKind>
    )>>;
    fn parser () -> Self::Parser {
        preceded(
            tag("Trace"),
            alt4(
                map(
                    preceded(
                        strip_whitespace(tag("Connect")),
                        strip_whitespace(AuthData::parser())
                    ),
                    |authdata| AppLogTraceKind::Connect(authdata)
                ),
                map(
                    preceded(
                        strip_whitespace(tag("SendRequest")),
                        strip_whitespace(unquote())
                    ),
                    |trace| AppLogTraceKind::SendRequest(trace)
                ),
                map(
                    preceded(
                        strip_whitespace(tag("Check")),
                        strip_whitespace(Announcements::parser())
                    ),
                    |announcements| AppLogTraceKind::Check(announcements)
                ),
                map(
                    preceded(
                        strip_whitespace(tag("GetResponse")),
                        strip_whitespace(unquote())
                    ),
                    |trace| AppLogTraceKind::GetResponse(trace)
                ),
            )
        )
    }
}
impl Parsable for AppLogJournalKind {
    type Parser = Preceded<Tag, Alt<(
        Map<Preceded<StripWhitespace<Tag>,Delimited<Tag,Permutation<(KeyValue<Unquote>,KeyValue<stdp::U32>)>,Tag>>,fn((String,u32))->AppLogJournalKind>,
        Map<Preceded<StripWhitespace<Tag>,Delimited<Tag,KeyValue<Unquote>,Tag>>,fn(String)->AppLogJournalKind>,
        Map<Preceded<StripWhitespace<Tag>,Delimited<Tag,Permutation<(KeyValue<Unquote>,KeyValue<Unquote>,KeyValue<stdp::U32>)>,Tag>>,fn((String,String,u32))->AppLogJournalKind>,
        Map<Preceded<StripWhitespace<Tag>,Delimited<Tag,Permutation<(KeyValue<Unquote>,KeyValue<Unquote>)>,Tag>>,fn((String,String))->AppLogJournalKind>,
        Map<Preceded<StripWhitespace<Tag>,<UserCash as Parsable>::Parser>,fn(UserCash)->AppLogJournalKind>,
        Map<Preceded<StripWhitespace<Tag>,<UserCash as Parsable>::Parser>,fn(UserCash)->AppLogJournalKind>,
        Map<Preceded<StripWhitespace<Tag>,<UserBacket as Parsable>::Parser>,fn(UserBacket)->AppLogJournalKind>,
        Map<Preceded<StripWhitespace<Tag>,<UserBacket as Parsable>::Parser>,fn(UserBacket)->AppLogJournalKind>,
    )>>;
    fn parser () -> Self::Parser {
        preceded(
            tag("Journal"),
            alt8(
                map(
                    preceded(
                        strip_whitespace(tag("CreateUser")),
                        delimited(tag("{"), permutation2(key_value("user_id", unquote()), key_value("authorized_capital", stdp::U32)), tag("}"))
                    ),
                    |(user_id,authorized_capital)| AppLogJournalKind::CreateUser{user_id,authorized_capital}
                ),
                map(
                    preceded(
                        strip_whitespace(tag("DeleteUser")),
                        delimited(tag("{"), key_value("user_id", unquote()), tag("}"))
                    ),
                    |user_id| AppLogJournalKind::DeleteUser{user_id}
                ),
                map(
                    preceded(
                        strip_whitespace(tag("RegisterAsset")),
                        delimited(tag("{"), permutation3(key_value("asset_id", unquote()), key_value("user_id", unquote()), key_value("liquidity", stdp::U32)), tag("}"))
                    ),
                    |(asset_id, user_id, liquidity)| AppLogJournalKind::RegisterAsset { asset_id, user_id, liquidity }
                ),
                map(
                    preceded(
                        strip_whitespace(tag("UnregisterAsset")),
                        delimited(tag("{"), permutation2(key_value("asset_id", unquote()), key_value("user_id", unquote())), tag("}"))
                    ),
                    |(asset_id, user_id)| AppLogJournalKind::UnregisterAsset { asset_id, user_id }
                ),
                map(
                    preceded(
                        strip_whitespace(tag("DepositCash")),
                        UserCash::parser()
                    ),
                    |user_cash| AppLogJournalKind::DepositCash (user_cash)
                ),
                map(
                    preceded(
                        strip_whitespace(tag("WithdrawCash")),
                        UserCash::parser()
                    ),
                    |user_cash| AppLogJournalKind::DepositCash (user_cash)
                ),
                map(
                    preceded(
                        strip_whitespace(tag("BuyAsset")),
                        UserBacket::parser()
                    ),
                    |user_backet| AppLogJournalKind::BuyAsset(user_backet)
                ),
                map(
                    preceded(
                        strip_whitespace(tag("SellAsset")),
                        UserBacket::parser()
                    ),
                    |user_backet| AppLogJournalKind::SellAsset(user_backet)
                ),
            )
        )
    }
}
impl Parsable for AppLogKind {
    type Parser = StripWhitespace<Preceded<Tag, Alt<(
        Map<<AppLogErrorKind as Parsable>::Parser,fn(AppLogErrorKind)->AppLogKind>,
        Map<<AppLogTraceKind as Parsable>::Parser,fn(AppLogTraceKind)->AppLogKind>,
        Map<<AppLogJournalKind as Parsable>::Parser,fn(AppLogJournalKind)->AppLogKind>,
    )>>>;
    fn parser () -> Self::Parser {
        strip_whitespace(preceded(
            tag("App::"),
            alt3(
                map(AppLogErrorKind::parser(), |error| AppLogKind::Error(error)),
                map(AppLogTraceKind::parser(), |trace| AppLogKind::Trace(trace)),
                map(AppLogJournalKind::parser(), |journal| AppLogKind::Journal(journal)),
            )
        ))
    }
}
impl Parsable for LogKind {
    type Parser = StripWhitespace<Alt<(
        Map<<SystemLogKind as Parsable>::Parser,fn(SystemLogKind)->LogKind>,
        Map<<AppLogKind as Parsable>::Parser,fn(AppLogKind)->LogKind>,
    )>>;
    fn parser () -> Self::Parser {
        strip_whitespace(alt2(
            map(SystemLogKind::parser(), |system| LogKind::System(system)),
            map(AppLogKind::parser(), |app| LogKind::App(app)),
        ))
    }
}
/// Строка логов, [лог](AppLogKind) с `request_id`
#[derive(Debug,Clone,PartialEq)]
pub struct LogLine {
    pub kind: LogKind,
    pub request_id: u32,
}
impl Parsable for LogLine {
    type Parser = Map<
        All<(<LogKind as Parsable>::Parser, StripWhitespace<Preceded<Tag,stdp::U32>>)>,
        fn((LogKind,u32))->Self
    >;
    fn parser () -> Self::Parser {
        map(
            all2(
                LogKind::parser(),
                strip_whitespace(preceded(tag("requestid="), stdp::U32))
            ),
            |(kind, request_id)| LogLine { kind, request_id }
        )
    }
}

/// Парсер строки логов
pub struct LogLineParser {
    parser: std::sync::OnceLock<<LogLine as Parsable>::Parser>,
}
impl LogLineParser {
    pub fn parse(&self, input: String) -> Result<(String, LogLine), ()> {
        self.parser.get_or_init(|| <LogLine as Parsable>::parser()).parse(input)
    }
}
// подсказка: singleton, без которого можно обойтись
// парсеры не страшно вытащить в pub
/// Единожды собранный парсер логов
pub static LOG_LINE_PARSER: LogLineParser = LogLineParser{parser: std::sync::OnceLock::new()};

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_u32() {
        assert_eq!(stdp::U32.parse("411".into()), Ok(("".into(), 411)));
        assert_eq!(stdp::U32.parse("411ab".into()), Ok(("ab".into(), 411)));
        assert_eq!(stdp::U32.parse("".into()), Err(()));
        assert_eq!(stdp::U32.parse("-3".into()), Err(()));
        assert_eq!(stdp::U32.parse("0x03".into()), Ok(("".into(), 0x3)));
        assert_eq!(stdp::U32.parse("0x03abg".into()), Ok(("g".into(), 0x3ab)));
        assert_eq!(stdp::U32.parse("0x".into()), Err(()));
    }

    #[test]
    fn test_i32() {
        assert_eq!(stdp::I32.parse("411".into()), Ok(("".into(), 411)));
        assert_eq!(stdp::I32.parse("411ab".into()), Ok(("ab".into(), 411)));
        assert_eq!(stdp::I32.parse("".into()), Err(()));
        assert_eq!(stdp::I32.parse("-3".into()), Ok(("".into(), -3)));
        assert_eq!(stdp::I32.parse("0x03".into()), Err(()));
        assert_eq!(stdp::I32.parse("-".into()), Err(()));
    }

    #[test]
    fn test_quote() {
        assert_eq!(quote(r#"411"#), r#""411""#.to_string());
        assert_eq!(quote(r#"4\11""#), r#""4\\11\"""#.to_string());
    }

    #[test]
    fn test_do_unquote_non_escaped() {
        assert_eq!(do_unquote_non_escaped(r#""411""#.into()), Ok(("".into(), "411".into())));
        assert_eq!(do_unquote_non_escaped(r#" "411""#.into()), Err(()));
        assert_eq!(do_unquote_non_escaped(r#"411"#.into()), Err(()));
    }

    #[test]
    fn test_unquote() {
        assert_eq!(Unquote.parse(r#""411""#.into()), Ok(("".into(), "411".into())));
        assert_eq!(Unquote.parse(r#" "411""#.into()), Err(()));
        assert_eq!(Unquote.parse(r#"411"#.into()), Err(()));

        assert_eq!(Unquote.parse(r#""ni\\c\"e""#.into()), Ok(("".into(), r#"ni\c"e"#.into())));
    }

    #[test]
    fn test_tag() {
        assert_eq!(tag("key=").parse("key=value".into()), Ok(("value".into(), ())));
        assert_eq!(tag("key=").parse("key:value".into()), Err(()));
    }

    #[test]
    fn test_quoted_tag() {
        assert_eq!(quoted_tag("key").parse(r#""key"=value"#.into()), Ok(("=value".into(), ())));
        assert_eq!(quoted_tag("key").parse(r#""key:"value"#.into()), Err(()));
        assert_eq!(quoted_tag("key").parse(r#"key=value"#.into()), Err(()));
    }

    #[test]
    fn test_strip_whitespace() {
        assert_eq!(strip_whitespace(tag("hello")).parse(" hello world".into()), Ok(("world".into(), ())));
        assert_eq!(strip_whitespace(tag("hello")).parse("hello".into()), Ok(("".into(), ())));
        assert_eq!(strip_whitespace(stdp::U32).parse(" 42 answer".into()), Ok(("answer".into(), 42)));
    }

    #[test]
    fn test_delimited() {
        assert_eq!(delimited(tag("["), stdp::U32, tag("]")).parse("[0x32]".into()), Ok(("".into(), 0x32)));
        assert_eq!(delimited(tag("[".into()), stdp::U32, tag("]".into())).parse("[0x32] nice".into()), Ok((" nice".into(), 0x32)));
        assert_eq!(delimited(tag("[".into()), stdp::U32, tag("]")).parse("0x32]".into()), Err(()));
        assert_eq!(delimited(tag("[".into()), stdp::U32, tag("]")).parse("[0x32".into()), Err(()));
    }

    #[test]
    fn test_key_value() {
        assert_eq!(key_value("key", stdp::U32).parse(r#""key":32,"#.into()), Ok(("".into(), 32)));
        assert_eq!(key_value("key", stdp::U32).parse(r#"key:32,"#.into()), Err(()));
        assert_eq!(key_value("key", stdp::U32).parse(r#""key":32"#.into()), Err(()));
        assert_eq!(key_value("key", stdp::U32).parse(r#" "key" : 32 , nice"#.into()), Ok(("nice".into(), 32)));
    }

    #[test]
    fn test_list() {
        assert_eq!(list(stdp::U32).parse("[1,2,3,4,]".into()), Ok(("".into(), vec![1, 2, 3, 4,])));
        assert_eq!(list(stdp::U32).parse(" [ 1 , 2 , 3 , 4 , ] nice".into()), Ok(("nice".into(), vec![1, 2, 3,4,])));
        assert_eq!(list(stdp::U32).parse("1,2,3,4,".into()), Err(()));
        assert_eq!(list(stdp::U32).parse("[]".into()), Ok(("".into(), vec![])));
    }

    #[test]
    fn test_authdata() {
        let s = "30c305825b900077ae7f8259c1c328aa3e124a07f3bfbbf216dfc6e308beea6e474b9a7ea6c24d003a6ae4fcf04a9e6ef7c7f17cdaa0296f66a88036badcf01f053da806fad356546349deceff24621b895440d05a715b221af8e9e068073d6dec04f148175717d3c2d1b6af84e2375718ab4a1eba7e037c1c1d43b4cf422d6f2aa9194266f0a7544eaeff8167f0e993d0ea6a8ddb98bfeb8805635d5ea9f6592fd5297e6f83b6834190f99449722cd0de87a4c122f08bbe836fd3092e5f0d37a3057e90f3dd41048da66cad3e8fd3ef72a9d86ecd9009c2db996af29dc62af5ef5eb04d0e16ce8fcecba92a4a9888f52d5d575e7dbc302ed97dbf69df15bb4f5c5601d38fbe3bd89d88768a6aed11ce2f95a6ad30bb72e787bfb734701cea1f38168be44ea19d3e98dd3c953fdb9951ac9c6e221bb0f980d8f0952ac8127da5bda7077dd25ffc8e1515c529f29516dacec6be9c084e6c91698267b2aed9038eca5ebafad479c5fb17652e25bb5b85586fae645bd7c3253d9916c0af65a20253412d5484ac15d288c6ca8823469090ded5ce0975dada63653797129f0e926af6247b457b067db683e37d848e0acf30e5602b78f1848e8da4b640ed08b75f3519a40ec96b2be964234beab37759504376c6e5ebfacdc57e4c7a22cf1e879d7bde29a2dca5fe20420215b59d102fd016606c533e8e36f7da114910664bade9b295d9043a01bc0dc4d8abbc16b1cec7789d89e699ad99dae597c7f10d6f047efc011d67444695cb8e6e8b3dba17ccc693729d01312d0f12a3fc76e12c2e4984af5cb3049b9d8a13124a1f770e96bae1fb153ba4c91bea4fae6f03010275d5a9b14012bdd678e037934dc6762005de54b32a7684e03060d5cc80378e9bef05b8f0692202944401bd06e4553e4490a0e57c5a72fc8abb1f714e22ea950fb2f1de284d6ff3da435954de355c677f60db4252a510919cbe7dadfed0441cf125fd8894753af8114f2ddacb75c3daa460920fc47d285e59fe9110e4151fcef03fa246cd2dd9a4d573e1dbbda1c6968cf4f546289b95ce1bf0a55eea6531382826d4002bc46bf441ce16056d42b5a2079e299e3191c23a7604cde03de6081e06f93cfe632c9a6088cd328662d47a4954934832df5b5f3765dbe136114c73c55cb7ce639e5d40d1d1d8f540d3c8e1bc7423f032c0da5264353468f009c973eec0448e41f9289e8d9dadc68da77d3c3ab3a6477d44024f21fba0bd4477d81c6027657527aa0413b45f417cb7b3beea835a1d5d795414d38156324cb5c1303e9924dbe40cd497c4c23c221cb912058c939bea8b79b3fea360fecaa83375a9a84e338d9e863e8021ad2df4430b8dea0c1714e1bdc478f559705549ad738453ab65c0ffcc8cf0e3bafaf4afad75ecc4dfad0de0cfe27d50d656456ea6c361b76508357714079424";
        let res = AuthData::parser().parse(s.to_string());
        assert!(res.is_ok());
        assert_eq!(res.as_ref().unwrap().0.len(), 0);
    }

    #[test]
    fn test_asset_dsc() {
        assert_eq!(all2(strip_whitespace(tag("AssetDsc")),strip_whitespace(tag("{"))).parse(" AssetDsc { ".into()), Ok(("".into(), ((),()))));

        assert_eq!(AssetDsc::parser().parse(r#"AssetDsc{"id":"usd","dsc":"USA dollar",}"#.into()), Ok(("".into(), AssetDsc{id: "usd".into(), dsc: "USA dollar".into()})));
        assert_eq!(AssetDsc::parser().parse(r#" AssetDsc { "id" : "usd" , "dsc" : "USA dollar" , } "#.into()), Ok(("".into(), AssetDsc{id: "usd".into(), dsc: "USA dollar".into()})));
        assert_eq!(AssetDsc::parser().parse(r#" AssetDsc { "id" : "usd" , "dsc" : "USA dollar" , } nice "#.into()), Ok(("nice ".into(), AssetDsc{id: "usd".into(), dsc: "USA dollar".into()})));

        assert_eq!(AssetDsc::parser().parse(r#"AssetDsc{"dsc":"USA dollar","id":"usd",}"#.into()), Ok(("".into(), AssetDsc{id: "usd".into(), dsc: "USA dollar".into()})));
    }

    #[test]
    fn test_backet() {
        assert_eq!(Backet::parser().parse(r#"Backet{"asset_id":"usd","count":42,}"#.into()), Ok(("".into(), Backet{asset_id: "usd".into(), count: 42})));
        assert_eq!(Backet::parser().parse(r#"Backet{"count":42,"asset_id":"usd",}"#.into()), Ok(("".into(), Backet{asset_id: "usd".into(), count: 42})));
    }

    #[test]
    fn test_log_kind() {
        assert_eq!(preceded(strip_whitespace(tag("NetworkError")), strip_whitespace(unquote())).parse(r#"NetworkError "url unknown""#.into()), Ok(("".into(), "url unknown".into())));

        assert_eq!(LogKind::parser().parse(r#"System::Error NetworkError "url unknown""#.into()), Ok(("".into(), LogKind::System(SystemLogKind::Error(SystemLogErrorKind::NetworkError("url unknown".into()))))));
        assert_eq!(LogKind::parser().parse(r#"App::Trace Connect 30c305825b900077ae7f8259c1c328aa3e124a07f3bfbbf216dfc6e308beea6e474b9a7ea6c24d003a6ae4fcf04a9e6ef7c7f17cdaa0296f66a88036badcf01f053da806fad356546349deceff24621b895440d05a715b221af8e9e068073d6dec04f148175717d3c2d1b6af84e2375718ab4a1eba7e037c1c1d43b4cf422d6f2aa9194266f0a7544eaeff8167f0e993d0ea6a8ddb98bfeb8805635d5ea9f6592fd5297e6f83b6834190f99449722cd0de87a4c122f08bbe836fd3092e5f0d37a3057e90f3dd41048da66cad3e8fd3ef72a9d86ecd9009c2db996af29dc62af5ef5eb04d0e16ce8fcecba92a4a9888f52d5d575e7dbc302ed97dbf69df15bb4f5c5601d38fbe3bd89d88768a6aed11ce2f95a6ad30bb72e787bfb734701cea1f38168be44ea19d3e98dd3c953fdb9951ac9c6e221bb0f980d8f0952ac8127da5bda7077dd25ffc8e1515c529f29516dacec6be9c084e6c91698267b2aed9038eca5ebafad479c5fb17652e25bb5b85586fae645bd7c3253d9916c0af65a20253412d5484ac15d288c6ca8823469090ded5ce0975dada63653797129f0e926af6247b457b067db683e37d848e0acf30e5602b78f1848e8da4b640ed08b75f3519a40ec96b2be964234beab37759504376c6e5ebfacdc57e4c7a22cf1e879d7bde29a2dca5fe20420215b59d102fd016606c533e8e36f7da114910664bade9b295d9043a01bc0dc4d8abbc16b1cec7789d89e699ad99dae597c7f10d6f047efc011d67444695cb8e6e8b3dba17ccc693729d01312d0f12a3fc76e12c2e4984af5cb3049b9d8a13124a1f770e96bae1fb153ba4c91bea4fae6f03010275d5a9b14012bdd678e037934dc6762005de54b32a7684e03060d5cc80378e9bef05b8f0692202944401bd06e4553e4490a0e57c5a72fc8abb1f714e22ea950fb2f1de284d6ff3da435954de355c677f60db4252a510919cbe7dadfed0441cf125fd8894753af8114f2ddacb75c3daa460920fc47d285e59fe9110e4151fcef03fa246cd2dd9a4d573e1dbbda1c6968cf4f546289b95ce1bf0a55eea6531382826d4002bc46bf441ce16056d42b5a2079e299e3191c23a7604cde03de6081e06f93cfe632c9a6088cd328662d47a4954934832df5b5f3765dbe136114c73c55cb7ce639e5d40d1d1d8f540d3c8e1bc7423f032c0da5264353468f009c973eec0448e41f9289e8d9dadc68da77d3c3ab3a6477d44024f21fba0bd4477d81c6027657527aa0413b45f417cb7b3beea835a1d5d795414d38156324cb5c1303e9924dbe40cd497c4c23c221cb912058c939bea8b79b3fea360fecaa83375a9a84e338d9e863e8021ad2df4430b8dea0c1714e1bdc478f559705549ad738453ab65c0ffcc8cf0e3bafaf4afad75ecc4dfad0de0cfe27d50d656456ea6c361b76508357714079424"#.into()), Ok(("".into(), LogKind::App(AppLogKind::Trace(AppLogTraceKind::Connect(AuthData([0x30,0xc3,0x05,0x82,0x5b,0x90,0x00,0x77,0xae,0x7f,0x82,0x59,0xc1,0xc3,0x28,0xaa,0x3e,0x12,0x4a,0x07,0xf3,0xbf,0xbb,0xf2,0x16,0xdf,0xc6,0xe3,0x08,0xbe,0xea,0x6e,0x47,0x4b,0x9a,0x7e,0xa6,0xc2,0x4d,0x00,0x3a,0x6a,0xe4,0xfc,0xf0,0x4a,0x9e,0x6e,0xf7,0xc7,0xf1,0x7c,0xda,0xa0,0x29,0x6f,0x66,0xa8,0x80,0x36,0xba,0xdc,0xf0,0x1f,0x05,0x3d,0xa8,0x06,0xfa,0xd3,0x56,0x54,0x63,0x49,0xde,0xce,0xff,0x24,0x62,0x1b,0x89,0x54,0x40,0xd0,0x5a,0x71,0x5b,0x22,0x1a,0xf8,0xe9,0xe0,0x68,0x07,0x3d,0x6d,0xec,0x04,0xf1,0x48,0x17,0x57,0x17,0xd3,0xc2,0xd1,0xb6,0xaf,0x84,0xe2,0x37,0x57,0x18,0xab,0x4a,0x1e,0xba,0x7e,0x03,0x7c,0x1c,0x1d,0x43,0xb4,0xcf,0x42,0x2d,0x6f,0x2a,0xa9,0x19,0x42,0x66,0xf0,0xa7,0x54,0x4e,0xae,0xff,0x81,0x67,0xf0,0xe9,0x93,0xd0,0xea,0x6a,0x8d,0xdb,0x98,0xbf,0xeb,0x88,0x05,0x63,0x5d,0x5e,0xa9,0xf6,0x59,0x2f,0xd5,0x29,0x7e,0x6f,0x83,0xb6,0x83,0x41,0x90,0xf9,0x94,0x49,0x72,0x2c,0xd0,0xde,0x87,0xa4,0xc1,0x22,0xf0,0x8b,0xbe,0x83,0x6f,0xd3,0x09,0x2e,0x5f,0x0d,0x37,0xa3,0x05,0x7e,0x90,0xf3,0xdd,0x41,0x04,0x8d,0xa6,0x6c,0xad,0x3e,0x8f,0xd3,0xef,0x72,0xa9,0xd8,0x6e,0xcd,0x90,0x09,0xc2,0xdb,0x99,0x6a,0xf2,0x9d,0xc6,0x2a,0xf5,0xef,0x5e,0xb0,0x4d,0x0e,0x16,0xce,0x8f,0xce,0xcb,0xa9,0x2a,0x4a,0x98,0x88,0xf5,0x2d,0x5d,0x57,0x5e,0x7d,0xbc,0x30,0x2e,0xd9,0x7d,0xbf,0x69,0xdf,0x15,0xbb,0x4f,0x5c,0x56,0x01,0xd3,0x8f,0xbe,0x3b,0xd8,0x9d,0x88,0x76,0x8a,0x6a,0xed,0x11,0xce,0x2f,0x95,0xa6,0xad,0x30,0xbb,0x72,0xe7,0x87,0xbf,0xb7,0x34,0x70,0x1c,0xea,0x1f,0x38,0x16,0x8b,0xe4,0x4e,0xa1,0x9d,0x3e,0x98,0xdd,0x3c,0x95,0x3f,0xdb,0x99,0x51,0xac,0x9c,0x6e,0x22,0x1b,0xb0,0xf9,0x80,0xd8,0xf0,0x95,0x2a,0xc8,0x12,0x7d,0xa5,0xbd,0xa7,0x07,0x7d,0xd2,0x5f,0xfc,0x8e,0x15,0x15,0xc5,0x29,0xf2,0x95,0x16,0xda,0xce,0xc6,0xbe,0x9c,0x08,0x4e,0x6c,0x91,0x69,0x82,0x67,0xb2,0xae,0xd9,0x03,0x8e,0xca,0x5e,0xba,0xfa,0xd4,0x79,0xc5,0xfb,0x17,0x65,0x2e,0x25,0xbb,0x5b,0x85,0x58,0x6f,0xae,0x64,0x5b,0xd7,0xc3,0x25,0x3d,0x99,0x16,0xc0,0xaf,0x65,0xa2,0x02,0x53,0x41,0x2d,0x54,0x84,0xac,0x15,0xd2,0x88,0xc6,0xca,0x88,0x23,0x46,0x90,0x90,0xde,0xd5,0xce,0x09,0x75,0xda,0xda,0x63,0x65,0x37,0x97,0x12,0x9f,0x0e,0x92,0x6a,0xf6,0x24,0x7b,0x45,0x7b,0x06,0x7d,0xb6,0x83,0xe3,0x7d,0x84,0x8e,0x0a,0xcf,0x30,0xe5,0x60,0x2b,0x78,0xf1,0x84,0x8e,0x8d,0xa4,0xb6,0x40,0xed,0x08,0xb7,0x5f,0x35,0x19,0xa4,0x0e,0xc9,0x6b,0x2b,0xe9,0x64,0x23,0x4b,0xea,0xb3,0x77,0x59,0x50,0x43,0x76,0xc6,0xe5,0xeb,0xfa,0xcd,0xc5,0x7e,0x4c,0x7a,0x22,0xcf,0x1e,0x87,0x9d,0x7b,0xde,0x29,0xa2,0xdc,0xa5,0xfe,0x20,0x42,0x02,0x15,0xb5,0x9d,0x10,0x2f,0xd0,0x16,0x60,0x6c,0x53,0x3e,0x8e,0x36,0xf7,0xda,0x11,0x49,0x10,0x66,0x4b,0xad,0xe9,0xb2,0x95,0xd9,0x04,0x3a,0x01,0xbc,0x0d,0xc4,0xd8,0xab,0xbc,0x16,0xb1,0xce,0xc7,0x78,0x9d,0x89,0xe6,0x99,0xad,0x99,0xda,0xe5,0x97,0xc7,0xf1,0x0d,0x6f,0x04,0x7e,0xfc,0x01,0x1d,0x67,0x44,0x46,0x95,0xcb,0x8e,0x6e,0x8b,0x3d,0xba,0x17,0xcc,0xc6,0x93,0x72,0x9d,0x01,0x31,0x2d,0x0f,0x12,0xa3,0xfc,0x76,0xe1,0x2c,0x2e,0x49,0x84,0xaf,0x5c,0xb3,0x04,0x9b,0x9d,0x8a,0x13,0x12,0x4a,0x1f,0x77,0x0e,0x96,0xba,0xe1,0xfb,0x15,0x3b,0xa4,0xc9,0x1b,0xea,0x4f,0xae,0x6f,0x03,0x01,0x02,0x75,0xd5,0xa9,0xb1,0x40,0x12,0xbd,0xd6,0x78,0xe0,0x37,0x93,0x4d,0xc6,0x76,0x20,0x05,0xde,0x54,0xb3,0x2a,0x76,0x84,0xe0,0x30,0x60,0xd5,0xcc,0x80,0x37,0x8e,0x9b,0xef,0x05,0xb8,0xf0,0x69,0x22,0x02,0x94,0x44,0x01,0xbd,0x06,0xe4,0x55,0x3e,0x44,0x90,0xa0,0xe5,0x7c,0x5a,0x72,0xfc,0x8a,0xbb,0x1f,0x71,0x4e,0x22,0xea,0x95,0x0f,0xb2,0xf1,0xde,0x28,0x4d,0x6f,0xf3,0xda,0x43,0x59,0x54,0xde,0x35,0x5c,0x67,0x7f,0x60,0xdb,0x42,0x52,0xa5,0x10,0x91,0x9c,0xbe,0x7d,0xad,0xfe,0xd0,0x44,0x1c,0xf1,0x25,0xfd,0x88,0x94,0x75,0x3a,0xf8,0x11,0x4f,0x2d,0xda,0xcb,0x75,0xc3,0xda,0xa4,0x60,0x92,0x0f,0xc4,0x7d,0x28,0x5e,0x59,0xfe,0x91,0x10,0xe4,0x15,0x1f,0xce,0xf0,0x3f,0xa2,0x46,0xcd,0x2d,0xd9,0xa4,0xd5,0x73,0xe1,0xdb,0xbd,0xa1,0xc6,0x96,0x8c,0xf4,0xf5,0x46,0x28,0x9b,0x95,0xce,0x1b,0xf0,0xa5,0x5e,0xea,0x65,0x31,0x38,0x28,0x26,0xd4,0x00,0x2b,0xc4,0x6b,0xf4,0x41,0xce,0x16,0x05,0x6d,0x42,0xb5,0xa2,0x07,0x9e,0x29,0x9e,0x31,0x91,0xc2,0x3a,0x76,0x04,0xcd,0xe0,0x3d,0xe6,0x08,0x1e,0x06,0xf9,0x3c,0xfe,0x63,0x2c,0x9a,0x60,0x88,0xcd,0x32,0x86,0x62,0xd4,0x7a,0x49,0x54,0x93,0x48,0x32,0xdf,0x5b,0x5f,0x37,0x65,0xdb,0xe1,0x36,0x11,0x4c,0x73,0xc5,0x5c,0xb7,0xce,0x63,0x9e,0x5d,0x40,0xd1,0xd1,0xd8,0xf5,0x40,0xd3,0xc8,0xe1,0xbc,0x74,0x23,0xf0,0x32,0xc0,0xda,0x52,0x64,0x35,0x34,0x68,0xf0,0x09,0xc9,0x73,0xee,0xc0,0x44,0x8e,0x41,0xf9,0x28,0x9e,0x8d,0x9d,0xad,0xc6,0x8d,0xa7,0x7d,0x3c,0x3a,0xb3,0xa6,0x47,0x7d,0x44,0x02,0x4f,0x21,0xfb,0xa0,0xbd,0x44,0x77,0xd8,0x1c,0x60,0x27,0x65,0x75,0x27,0xaa,0x04,0x13,0xb4,0x5f,0x41,0x7c,0xb7,0xb3,0xbe,0xea,0x83,0x5a,0x1d,0x5d,0x79,0x54,0x14,0xd3,0x81,0x56,0x32,0x4c,0xb5,0xc1,0x30,0x3e,0x99,0x24,0xdb,0xe4,0x0c,0xd4,0x97,0xc4,0xc2,0x3c,0x22,0x1c,0xb9,0x12,0x05,0x8c,0x93,0x9b,0xea,0x8b,0x79,0xb3,0xfe,0xa3,0x60,0xfe,0xca,0xa8,0x33,0x75,0xa9,0xa8,0x4e,0x33,0x8d,0x9e,0x86,0x3e,0x80,0x21,0xad,0x2d,0xf4,0x43,0x0b,0x8d,0xea,0x0c,0x17,0x14,0xe1,0xbd,0xc4,0x78,0xf5,0x59,0x70,0x55,0x49,0xad,0x73,0x84,0x53,0xab,0x65,0xc0,0xff,0xcc,0x8c,0xf0,0xe3,0xba,0xfa,0xf4,0xaf,0xad,0x75,0xec,0xc4,0xdf,0xad,0x0d,0xe0,0xcf,0xe2,0x7d,0x50,0xd6,0x56,0x45,0x6e,0xa6,0xc3,0x61,0xb7,0x65,0x08,0x35,0x77,0x14,0x07,0x94,0x24])))))));
        assert_eq!(LogKind::parser().parse(r#"App::Journal CreateUser {"user_id": "Steeve", "authorized_capital": 10000,}"#.into()), Ok(("".into(), LogKind::App(AppLogKind::Journal(AppLogJournalKind::CreateUser{user_id: "Steeve".into(), authorized_capital: 10_000})))));
        assert_eq!(LogKind::parser().parse(r#"App::Journal DeleteUser {"user_id": "Steeve",}"#.into()), Ok(("".into(), LogKind::App(AppLogKind::Journal(AppLogJournalKind::DeleteUser{user_id: "Steeve".into()})))));
        assert_eq!(LogKind::parser().parse(r#"App::Journal RegisterAsset {"asset_id": "bayc", "liquidity": 100000000, "user_id": "Steeve",}"#.into()), Ok(("".into(), LogKind::App(AppLogKind::Journal(AppLogJournalKind::RegisterAsset{asset_id: "bayc".into(), user_id: "Steeve".into(), liquidity: 100_000_000})))));
        assert_eq!(LogKind::parser().parse(r#"App::Journal DepositCash UserCash{"user_id": "Steeve", "count": 10,}"#.into()), Ok(("".into(), LogKind::App(AppLogKind::Journal(AppLogJournalKind::DepositCash(UserCash{user_id: "Steeve".into(), count: 10}))))));
        assert_eq!(LogKind::parser().parse(r#"App::Journal BuyAsset UserBacket{"user_id": "Steeve", "backet": Backet{"asset_id":"bayc","count":1,},}"#.into()), Ok(("".into(), LogKind::App(AppLogKind::Journal(AppLogJournalKind::BuyAsset(UserBacket{user_id: "Steeve".into(), backet: Backet{asset_id: "bayc".into(),count:1}}))))));
    }
}
