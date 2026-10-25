// Install smoke fixture (wicked-estate#360): a known call for `blast-radius smoke_leaf`.
pub fn smoke_leaf() -> u32 {
    1
}

pub fn smoke_caller() -> u32 {
    smoke_leaf() + 1
}
