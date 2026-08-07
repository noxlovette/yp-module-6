// Пусть есть логи:
// System(requestid):
// - trace
// - error
// App(requestid):
// - trace
// - error
// - journal (человекочитаемая сводка)

// Есть прототип штуки, которая умеет:
// - парсить логи
// - фильтровать
//  -- по requestid
//  -- по ошибкам
//  -- по изменению счёта (купить/продать)

use std::{fs::File, io::BufReader};

// Модель данных:
// - Пользователь (userid, имя)
// - Вещи
//  -- Предмет (assetid, название)
//  -- Набор (assetid, количество)
//      comment{-- Собственность (assetid, userid владельца, количество)}
//  -- Таблица предложения (assetid на assetid, userid продавца)
//  -- Таблица спроса (assetid на assetid, userid покупателя)
// - Операция App
//  -- Journal
//   --- Создать пользователя userid с уставным капиталом от 10usd и выше
//   --- Удалить пользователя
//   --- Зарегистрировать assetid с ликвидностью от 50usd
//   --- Удалить assetid (весь asset должен принадлежать пользователю)
//   --- Внести usd для userid (usd (aka доллар сша) - это тип asset)
//   --- Вывести usd для userid
//   --- Купить asset
//   --- Продать asset
//  -- Trace
//   --- Соединить с биржей
//   --- Получить данные с биржи
//   --- Локальная проверка корректности (упреждение ошибок в ответе)
//   --- Отправить запрос в биржу
//   --- Получить ответ от биржи
//  -- Error
//   --- нет asset
//   --- системная ошибка
// - Операция System
//  -- Trace
//   --- Отправить запрос
//   --- Получить ответ
//  -- Error
//   --- нет сети
//   --- отказано в доступе
fn main() -> anyhow::Result<()> {
    println!("Placeholder для экспериментов с cli");

    let parsing_demo = r#"[UserBuckets{"user_id":"Bob","buckets":[Bucket{"asset_id":"milk","count":3,},],},]"#;
    let announcements = analysis::parse::just_parse::<
        analysis::domain::Announcements,
    >(parsing_demo)?;
    println!("demo-parsed: {:?}", announcements);

    let args = std::env::args().collect::<Vec<_>>();
    let path = &args[1];

    println!(
        "Trying to open file '{}' from directory '{}'",
        &path,
        std::env::current_dir()?.to_string_lossy()
    );
    let f = File::open(&path)?;
    let file = BufReader::new(f);

    let logs = analysis::read_log(file, analysis::ReadMode::All, vec![]);

    println!("got logs:");
    logs.iter().for_each(|parsed| println!("  {:?}", parsed));

    Ok(())
}
