use std::time::{Duration, Instant};

use sheetgaze::domain::row_store::LazyConfig;
use sheetgaze::io::csv::load_csv;

#[test]
#[ignore = "performance check; run with --release -- --ignored"]
fn csv_100k_rows_loads_within_two_seconds() {
    let path = std::env::temp_dir().join("sheetgaze-100k.csv");
    let mut text = String::from("id,name,value\n");
    for row in 0..100_000 {
        text.push_str(&format!("{row},item-{row},{row}\n"));
    }
    std::fs::write(&path, text).unwrap();
    let start = Instant::now();
    let workbook = load_csv(
        &path,
        &Default::default(),
        &LazyConfig {
            enabled: false,
            ..LazyConfig::default()
        },
        None,
    )
    .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(workbook.sheets[0].meta.row_count, 100_001);
    assert!(
        elapsed < Duration::from_secs(2),
        "100k CSV load took {elapsed:?}"
    );
}
