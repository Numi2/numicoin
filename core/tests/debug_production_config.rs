use numi_core::config::Config;

#[test]
fn debug_production_config() {
    let cfg = Config::production();
    let res = cfg.validate();
    println!("validate result: {:?}", res);
    if let Err(e) = res {
        panic!("validation error: {}", e);
    }
}
