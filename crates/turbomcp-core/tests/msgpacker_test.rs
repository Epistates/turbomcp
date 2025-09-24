#[cfg(feature = "messagepack")]
#[test]
fn explore_msgpacker_api() {
    use msgpacker::Packable;

    // Test basic types that msgpacker can handle
    let mut buf = Vec::new();

    // Test string
    let test_string = "hello";
    let bytes_written = test_string.pack(&mut buf);
    println!("String packed {} bytes", bytes_written);

    buf.clear();

    // Test number
    let test_num = 42i32;
    let bytes_written = test_num.pack(&mut buf);
    println!("Number packed {} bytes", bytes_written);

    buf.clear();

    // Test boolean
    let test_bool = true;
    let bytes_written = test_bool.pack(&mut buf);
    println!("Boolean packed {} bytes", bytes_written);

    buf.clear();

    // Test vector
    let test_vec = vec![1, 2, 3];
    let bytes_written = test_vec.pack(&mut buf);
    println!("Vec packed {} bytes", bytes_written);
}
