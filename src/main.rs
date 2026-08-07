use std::{fs::File, io::BufReader};

fn main() -> anyhow::Result<()> {
    println!("Placeholder для экспериментов с cli");

    let parsing_demo = r#"[UserBuckets{"user_id":"Bob","buckets":[Bucket{"asset_id":"milk","count":3,},],},]"#;
    let announcements = analysis::parse::just_parse::<
        analysis::domain::Announcements,
    >(parsing_demo)?;
    println!("demo-parsed: {:?}", announcements);

    let args = std::env::args().collect::<Vec<_>>();
    if let Some(path) = args.iter().skip(1).next() {
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
    } else {
        anyhow::bail!("Got empty command list")
    }
}
