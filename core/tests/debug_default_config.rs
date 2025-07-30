#[test]
fn debug_default_config() {
    let cfg = numi_core::config::Config::default();
    match cfg.validate() {
        Ok(_) => println!("validate OK"),
        Err(e) => println!("validate ERR: {}", e),
    }

    println!("network.enabled = {}", cfg.network.enabled);
    println!("network.listen_port = {}", cfg.network.listen_port);
    println!("rpc.port = {}", cfg.rpc.port);
}
