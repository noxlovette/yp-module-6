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
fn main() {
    println!("Placeholder для экспериментов с cli");

    let parsing_demo = r#"[UserBackets{"user_id":"Bob","backets":[Backet{"asset_id":"milk","count":3,},],},]"#.to_string();
    let announcements = analysis::parse::just_parse_anouncements(parsing_demo).unwrap();
    println!("demo-parsed: {:?}", announcements);

    let args = std::env::args().collect::<Vec<_>>();
    let filename = args[1].clone();
    println!("Trying opening file '{}' from directory '{}'", filename, std::env::current_dir().unwrap().to_string_lossy());
    let file: std::rc::Rc<std::cell::RefCell<Box<dyn analysis::MyReader>>> = std::rc::Rc::new(std::cell::RefCell::new(Box::new(std::fs::File::open(filename).unwrap())));

    let logs = analysis::read_log(file.clone(), analysis::READ_MODE_ALL, vec![]);
    println!("got logs:");
    logs.iter().for_each(|parsed| println!("  {:?}", parsed));
}

