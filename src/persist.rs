//! 영구(재부팅 후에도 유지되는) IPv4 라우트.
//!
//! `route -p add`와 같은 저장소를 쓴다:
//! `HKLM\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\PersistentRoutes` 아래에
//! 값 이름이 `대상,넷마스크,게이트웨이,메트릭`인 빈 REG_SZ 값으로 저장되며,
//! 부팅 시 Windows가 이를 읽어 활성 테이블에 올린다. 인터페이스 정보는 저장되지 않는다.

use std::net::Ipv4Addr;

use windows_registry::LOCAL_MACHINE;

const KEY_PATH: &str = r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\PersistentRoutes";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistentRoute {
    pub dest: Ipv4Addr,
    pub mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub metric: i32,
    /// 레지스트리 값 이름 원문 (삭제 시 사용).
    pub value_name: String,
}

impl PersistentRoute {
    pub fn matches(&self, dest: Ipv4Addr, mask: Ipv4Addr, gateway: Ipv4Addr) -> bool {
        self.dest == dest && self.mask == mask && self.gateway == gateway
    }
}

/// `10.0.0.0,255.0.0.0,192.168.0.1,1` 형식의 값 이름을 해석한다.
pub fn parse_value_name(name: &str) -> Option<PersistentRoute> {
    let mut parts = name.split(',').map(str::trim);
    let dest = parts.next()?.parse().ok()?;
    let mask = parts.next()?.parse().ok()?;
    let gateway = parts.next()?.parse().ok()?;
    let metric = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(PersistentRoute {
        dest,
        mask,
        gateway,
        metric,
        value_name: name.to_owned(),
    })
}

pub fn format_value_name(dest: Ipv4Addr, mask: Ipv4Addr, gateway: Ipv4Addr, metric: u32) -> String {
    format!("{dest},{mask},{gateway},{metric}")
}

/// 저장된 영구 라우트 목록. 키가 없으면 빈 목록.
pub fn list() -> Result<Vec<PersistentRoute>, String> {
    let Ok(key) = LOCAL_MACHINE.open(KEY_PATH) else {
        return Ok(Vec::new());
    };
    let values = key
        .values()
        .map_err(|e| format!("영구 라우트 조회 실패: {}", e.message()))?;
    let mut routes: Vec<PersistentRoute> = values
        .filter_map(|(name, _)| parse_value_name(&name))
        .collect();
    routes.sort_by_key(|r| (r.dest, r.mask, r.gateway));
    Ok(routes)
}

/// 영구 라우트를 저장한다. 같은 대상/마스크/게이트웨이 항목이 있으면 교체한다.
pub fn add(dest: Ipv4Addr, mask: Ipv4Addr, gateway: Ipv4Addr, metric: u32) -> Result<(), String> {
    let existing = list()?;
    let result = (|| {
        let key = LOCAL_MACHINE
            .options()
            .read()
            .write()
            .create()
            .open(KEY_PATH)?;
        for old in existing.iter().filter(|r| r.matches(dest, mask, gateway)) {
            key.remove_value(&old.value_name)?;
        }
        key.set_string(format_value_name(dest, mask, gateway, metric), "")
    })();
    result.map_err(|e| format!("영구 라우트 저장 실패: {}", e.message()))
}

/// 영구 라우트 항목을 지운다 (활성 테이블은 건드리지 않음).
pub fn remove(value_name: &str) -> Result<(), String> {
    LOCAL_MACHINE
        .options()
        .read()
        .write()
        .open(KEY_PATH)
        .and_then(|key| key.remove_value(value_name))
        .map_err(|e| format!("영구 라우트 삭제 실패: {}", e.message()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_name_roundtrip() {
        let name = format_value_name(
            Ipv4Addr::new(10, 0, 0, 0),
            Ipv4Addr::new(255, 0, 0, 0),
            Ipv4Addr::new(192, 168, 0, 1),
            5,
        );
        assert_eq!(name, "10.0.0.0,255.0.0.0,192.168.0.1,5");
        let parsed = parse_value_name(&name).unwrap();
        assert_eq!(parsed.dest, Ipv4Addr::new(10, 0, 0, 0));
        assert_eq!(parsed.mask, Ipv4Addr::new(255, 0, 0, 0));
        assert_eq!(parsed.gateway, Ipv4Addr::new(192, 168, 0, 1));
        assert_eq!(parsed.metric, 5);
        assert_eq!(parsed.value_name, name);
    }

    #[test]
    fn value_name_variants() {
        // 고정 IP 설정의 기본 게이트웨이는 메트릭 -1로 저장되기도 한다.
        assert_eq!(
            parse_value_name("0.0.0.0,0.0.0.0,192.168.0.1,-1")
                .unwrap()
                .metric,
            -1
        );
        assert!(parse_value_name("10.0.0.0,255.0.0.0,192.168.0.1").is_none());
        assert!(parse_value_name("10.0.0.0,255.0.0.0,192.168.0.1,1,extra").is_none());
        assert!(parse_value_name("garbage").is_none());
    }

    /// 실제 레지스트리를 읽기만 한다.
    #[test]
    fn lists_without_error() {
        list().expect("list");
    }
}
