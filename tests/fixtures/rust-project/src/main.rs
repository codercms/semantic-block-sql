use semblock_rust_fixture::{orders, users};

fn main() {
    println!("{} {}", users::find_by_id().len(), orders::queries().len());
}
