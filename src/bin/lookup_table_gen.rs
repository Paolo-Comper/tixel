fn main() {
    println!("const LUT: [&str; 256] = [");

    for i in 0..256 {
        println!("    \"{}\",", i);
    }

    println!("];");
}
