mutself::mutself! {
    MY_DATA_NUM = 0xDEADBEEF_usize;
    FILE = *include_bytes!("Cargo.toml");
}

pub fn main() {
    dbg!(&*MY_DATA_NUM);
    if let Some(arg) = std::env::args().nth(1) {
        println!("{arg}");
        mutself(
            "new.exe",
            Some(&arg.parse::<usize>().unwrap().to_ne_bytes()),
            None,
        )
        .unwrap();
    }
}
