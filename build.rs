// exe에 아이콘과 버전 정보 리소스를 넣는다 (탐색기 "속성 > 자세히"에 보이는 값).
fn main() {
    println!("cargo:rerun-if-changed=assets/routeworker.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/routeworker.ico")
        .set("ProductName", "RouteWorker")
        .set("FileDescription", "RouteWorker")
        .set("CompanyName", "DHPLab Consulting")
        .set("LegalCopyright", "Copyright (c) 2026 DHPLab Consulting")
        .set("OriginalFilename", "RouteWorker.exe");
    res.compile().expect("Windows 리소스 컴파일 실패");
}
