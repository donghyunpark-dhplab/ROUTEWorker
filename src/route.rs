//! Windows IP Helper API 래퍼: 라우팅 테이블 조회/추가/수정/삭제.
//!
//! `route.exe` 출력을 파싱하지 않고 `GetIpForwardTable2` 계열 API를 직접 호출한다.
//! 메트릭 주의: API의 `Metric`은 "라우트 메트릭"이고, `route print`에 보이는 값은
//! 여기에 인터페이스 메트릭을 더한 값이다.

use std::ffi::c_void;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER, ERROR_NOT_FOUND,
    ERROR_OBJECT_ALREADY_EXISTS, WIN32_ERROR,
};
use windows::Win32::NetworkManagement::IpHelper::{
    ConvertInterfaceLuidToAlias, CreateIpForwardEntry2, DeleteIpForwardEntry2, FreeMibTable,
    GetIpForwardTable2, GetIpInterfaceTable, GetUnicastIpAddressTable, InitializeIpForwardEntry,
    MIB_IPFORWARD_ROW2, MIB_IPFORWARD_TABLE2, MIB_IPINTERFACE_ROW, MIB_IPINTERFACE_TABLE,
    MIB_UNICASTIPADDRESS_ROW, MIB_UNICASTIPADDRESS_TABLE, SetIpForwardEntry2,
};
use windows::Win32::NetworkManagement::Ndis::NET_LUID_LH;
use windows::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, AF_UNSPEC, MIB_IPPROTO_NETMGMT, SOCKADDR_INET,
};

/// 라우트를 식별하는 키 (대상 프리픽스 + 다음 홉 + 인터페이스).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RouteKey {
    pub dest: IpAddr,
    pub prefix_len: u8,
    pub next_hop: IpAddr,
    pub if_index: u32,
}

/// 활성 라우팅 테이블의 한 항목.
#[derive(Clone)]
pub struct Route {
    pub dest: IpAddr,
    pub prefix_len: u8,
    /// 다음 홉. on-link 라우트는 `0.0.0.0` / `::`.
    pub next_hop: IpAddr,
    pub if_index: u32,
    /// 라우트 메트릭 (인터페이스 메트릭 미포함).
    pub metric: u32,
    pub protocol: i32,
    pub origin: i32,
    /// 수정/삭제 시 그대로 API에 돌려주기 위한 원본 행.
    raw: MIB_IPFORWARD_ROW2,
}

impl Route {
    pub fn key(&self) -> RouteKey {
        RouteKey {
            dest: self.dest,
            prefix_len: self.prefix_len,
            next_hop: self.next_hop,
            if_index: self.if_index,
        }
    }

    pub fn is_ipv4(&self) -> bool {
        self.dest.is_ipv4()
    }

    pub fn is_on_link(&self) -> bool {
        self.next_hop.is_unspecified()
    }

    pub fn is_default(&self) -> bool {
        self.prefix_len == 0
    }

    /// 사용자가 수동으로 추가한 정적 라우트인지 (`kind_text`의 "정적"과 같은 조건).
    pub fn is_static(&self) -> bool {
        self.origin != 2 && self.origin != 3 && self.protocol == 3
    }

    /// `10.0.0.0/8` 형식.
    pub fn dest_text(&self) -> String {
        format!("{}/{}", self.dest, self.prefix_len)
    }

    /// IPv4는 점 표기 넷마스크, IPv6는 프리픽스 길이.
    pub fn mask_text(&self) -> String {
        match self.dest {
            IpAddr::V4(_) => prefix_to_mask_v4(self.prefix_len).to_string(),
            IpAddr::V6(_) => format!("/{}", self.prefix_len),
        }
    }

    /// 라우트가 어디서 왔는지 (정적/시스템/DHCP/RA ...).
    pub fn kind_text(&self) -> String {
        match (self.origin, self.protocol) {
            (3, _) => "RA".to_owned(),
            (2, _) | (_, 19) => "DHCP".to_owned(),
            (_, 2) => "시스템".to_owned(),
            (_, 3) => "정적".to_owned(),
            (_, 4) => "ICMP".to_owned(),
            (_, p) => format!("기타({p})"),
        }
    }
}

/// 네트워크 인터페이스 (IPv4/IPv6 정보를 인덱스 기준으로 합친 것).
#[derive(Clone, Debug, Default)]
pub struct Interface {
    pub index: u32,
    pub alias: String,
    pub metric_v4: Option<u32>,
    pub metric_v6: Option<u32>,
    pub connected: bool,
    /// (주소, on-link 프리픽스 길이)
    pub addrs: Vec<(IpAddr, u8)>,
}

impl Interface {
    pub fn metric_for(&self, ipv4: bool) -> Option<u32> {
        if ipv4 { self.metric_v4 } else { self.metric_v6 }
    }

    pub fn first_ipv4(&self) -> Option<Ipv4Addr> {
        self.addrs.iter().find_map(|(a, _)| match a {
            IpAddr::V4(v4) => Some(*v4),
            IpAddr::V6(_) => None,
        })
    }

    /// 콤보박스 등에 쓰는 표시 이름.
    pub fn label(&self) -> String {
        match self.first_ipv4() {
            Some(ip) => format!("{} — {} [#{}]", self.alias, ip, self.index),
            None => format!("{} [#{}]", self.alias, self.index),
        }
    }
}

/// 새로 만들 라우트의 사양.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteSpec {
    pub dest: IpAddr,
    pub prefix_len: u8,
    /// `None`이면 on-link (게이트웨이 없음).
    pub gateway: Option<IpAddr>,
    pub if_index: u32,
    pub metric: u32,
}

impl RouteSpec {
    pub fn next_hop(&self) -> IpAddr {
        self.gateway.unwrap_or(match self.dest {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        })
    }

    pub fn key(&self) -> RouteKey {
        RouteKey {
            dest: self.dest,
            prefix_len: self.prefix_len,
            next_hop: self.next_hop(),
            if_index: self.if_index,
        }
    }
}

// ---------------------------------------------------------------------------
// 조회
// ---------------------------------------------------------------------------

/// `FreeMibTable`로 해제해야 하는 테이블 포인터의 RAII 가드.
struct MibTable<T>(*mut T);

impl<T> Drop for MibTable<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { FreeMibTable(self.0 as *const c_void) };
        }
    }
}

/// 활성 라우팅 테이블 전체(IPv4 + IPv6)를 읽는다.
pub fn list_routes() -> Result<Vec<Route>, String> {
    let mut table = MibTable::<MIB_IPFORWARD_TABLE2>(std::ptr::null_mut());
    check(
        unsafe { GetIpForwardTable2(AF_UNSPEC, &mut table.0) },
        "라우팅 테이블 조회",
    )?;

    // SAFETY: 성공 시 table.0은 NumEntries개의 행이 뒤따르는 유효한 테이블을 가리킨다.
    let rows = unsafe {
        let first = std::ptr::addr_of!((*table.0).Table) as *const MIB_IPFORWARD_ROW2;
        std::slice::from_raw_parts(first, (*table.0).NumEntries as usize)
    };

    Ok(rows
        .iter()
        .filter_map(|row| {
            let dest = sockaddr_to_ip(&row.DestinationPrefix.Prefix)?;
            let next_hop = sockaddr_to_ip(&row.NextHop).unwrap_or(unspecified_like(dest));
            Some(Route {
                dest,
                prefix_len: row.DestinationPrefix.PrefixLength,
                next_hop,
                if_index: row.InterfaceIndex,
                metric: row.Metric,
                protocol: row.Protocol.0,
                origin: row.Origin.0,
                raw: *row,
            })
        })
        .collect())
}

/// 인터페이스 목록 (별칭, 메트릭, 주소 포함).
pub fn list_interfaces() -> Result<Vec<Interface>, String> {
    let mut out: Vec<Interface> = Vec::new();

    {
        let mut table = MibTable::<MIB_IPINTERFACE_TABLE>(std::ptr::null_mut());
        check(
            unsafe { GetIpInterfaceTable(AF_UNSPEC, &mut table.0) },
            "인터페이스 조회",
        )?;
        // SAFETY: 위와 동일.
        let rows = unsafe {
            let first = std::ptr::addr_of!((*table.0).Table) as *const MIB_IPINTERFACE_ROW;
            std::slice::from_raw_parts(first, (*table.0).NumEntries as usize)
        };

        for row in rows {
            let pos = match out.iter().position(|i| i.index == row.InterfaceIndex) {
                Some(pos) => pos,
                None => {
                    out.push(Interface {
                        index: row.InterfaceIndex,
                        alias: interface_alias(&row.InterfaceLuid)
                            .unwrap_or_else(|| format!("인터페이스 {}", row.InterfaceIndex)),
                        ..Default::default()
                    });
                    out.len() - 1
                }
            };
            let iface = &mut out[pos];
            if row.Family == AF_INET {
                iface.metric_v4 = Some(row.Metric);
            } else if row.Family == AF_INET6 {
                iface.metric_v6 = Some(row.Metric);
            }
            iface.connected |= row.Connected;
        }
    }

    {
        let mut table = MibTable::<MIB_UNICASTIPADDRESS_TABLE>(std::ptr::null_mut());
        check(
            unsafe { GetUnicastIpAddressTable(AF_UNSPEC, &mut table.0) },
            "주소 조회",
        )?;
        // SAFETY: 위와 동일.
        let rows = unsafe {
            let first = std::ptr::addr_of!((*table.0).Table) as *const MIB_UNICASTIPADDRESS_ROW;
            std::slice::from_raw_parts(first, (*table.0).NumEntries as usize)
        };
        for row in rows {
            if let (Some(ip), Some(iface)) = (
                sockaddr_to_ip(&row.Address),
                out.iter_mut().find(|i| i.index == row.InterfaceIndex),
            ) {
                iface.addrs.push((ip, row.OnLinkPrefixLength));
            }
        }
    }

    out.sort_by_key(|i| i.index);
    Ok(out)
}

fn interface_alias(luid: &NET_LUID_LH) -> Option<String> {
    let mut buf = [0u16; 257];
    let rc = unsafe { ConvertInterfaceLuidToAlias(luid, &mut buf) };
    if rc.is_err() {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

// ---------------------------------------------------------------------------
// 변경
// ---------------------------------------------------------------------------

/// 활성 라우팅 테이블에 라우트를 추가한다 (재부팅 시 사라짐).
pub fn add_route(spec: &RouteSpec) -> Result<(), String> {
    let mut row = MIB_IPFORWARD_ROW2::default();
    unsafe { InitializeIpForwardEntry(&mut row) };
    row.InterfaceIndex = spec.if_index;
    row.DestinationPrefix.Prefix = SOCKADDR_INET::from(SocketAddr::new(spec.dest, 0));
    row.DestinationPrefix.PrefixLength = spec.prefix_len;
    row.NextHop = SOCKADDR_INET::from(SocketAddr::new(spec.next_hop(), 0));
    row.Metric = spec.metric;
    row.Protocol = MIB_IPPROTO_NETMGMT;
    check(unsafe { CreateIpForwardEntry2(&row) }, "라우트 추가")
}

/// 활성 라우팅 테이블에서 라우트를 삭제한다.
pub fn delete_route(route: &Route) -> Result<(), String> {
    check(unsafe { DeleteIpForwardEntry2(&route.raw) }, "라우트 삭제")
}

/// 기존 라우트의 메트릭만 바꾼다 (대상/게이트웨이/인터페이스는 그대로).
pub fn set_metric(route: &Route, metric: u32) -> Result<(), String> {
    let mut row = route.raw;
    row.Metric = metric;
    check(unsafe { SetIpForwardEntry2(&row) }, "메트릭 변경")
}

fn check(rc: WIN32_ERROR, what: &str) -> Result<(), String> {
    if rc.is_ok() {
        return Ok(());
    }
    let hint = if rc == ERROR_ACCESS_DENIED {
        "관리자 권한이 필요합니다.".to_owned()
    } else if rc == ERROR_OBJECT_ALREADY_EXISTS {
        "같은 라우트가 이미 존재합니다.".to_owned()
    } else if rc == ERROR_NOT_FOUND || rc == ERROR_FILE_NOT_FOUND {
        "해당 라우트(또는 인터페이스)를 찾을 수 없습니다.".to_owned()
    } else if rc == ERROR_INVALID_PARAMETER {
        "매개 변수가 올바르지 않습니다. 대상/게이트웨이/인터페이스 조합을 확인하세요.".to_owned()
    } else {
        windows::core::Error::from(rc.to_hresult()).message()
    };
    Err(format!("{what} 실패: {hint} (오류 {})", rc.0))
}

// ---------------------------------------------------------------------------
// 주소 유틸리티 (순수 함수)
// ---------------------------------------------------------------------------

fn sockaddr_to_ip(sa: &SOCKADDR_INET) -> Option<IpAddr> {
    // SAFETY: si_family로 어느 union 멤버가 유효한지 판별한다.
    unsafe {
        if sa.si_family == AF_INET {
            Some(IpAddr::V4(Ipv4Addr::from(sa.Ipv4.sin_addr)))
        } else if sa.si_family == AF_INET6 {
            Some(IpAddr::V6(Ipv6Addr::from(sa.Ipv6.sin6_addr)))
        } else {
            None
        }
    }
}

fn unspecified_like(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    }
}

pub fn prefix_to_mask_v4(prefix_len: u8) -> Ipv4Addr {
    let bits = match prefix_len {
        0 => 0,
        n if n >= 32 => u32::MAX,
        n => u32::MAX << (32 - n),
    };
    Ipv4Addr::from(bits)
}

/// 연속된 1비트로 이루어진 넷마스크만 허용한다.
pub fn mask_to_prefix_v4(mask: Ipv4Addr) -> Option<u8> {
    let bits = u32::from(mask);
    let ones = bits.leading_ones();
    (bits.checked_shl(ones).unwrap_or(0) == 0).then_some(ones as u8)
}

/// 호스트 비트를 0으로 만든 네트워크 주소.
pub fn network_addr(ip: IpAddr, prefix_len: u8) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => IpAddr::V4(Ipv4Addr::from(
            u32::from(v4) & u32::from(prefix_to_mask_v4(prefix_len)),
        )),
        IpAddr::V6(v6) => {
            let mask = match prefix_len {
                0 => 0,
                n if n >= 128 => u128::MAX,
                n => u128::MAX << (128 - n),
            };
            IpAddr::V6(Ipv6Addr::from(u128::from(v6) & mask))
        }
    }
}

fn max_prefix(ip: IpAddr) -> u8 {
    if ip.is_ipv4() { 32 } else { 128 }
}

/// 사용자 입력(대상 + 넷마스크/프리픽스)을 정규화된 `(네트워크 주소, 프리픽스 길이)`로 바꾼다.
///
/// - 대상은 `10.0.0.0` 또는 `10.0.0.0/8` 형식.
/// - 마스크는 `255.0.0.0`(IPv4) 또는 `8` 형식. 비우면 호스트 라우트(/32, /128).
/// - 호스트 비트가 섞여 있으면 네트워크 주소로 잘라낸다.
pub fn parse_destination(dest: &str, mask: &str) -> Result<(IpAddr, u8), String> {
    let dest = dest.trim();
    let mask = mask.trim();
    if dest.is_empty() {
        return Err("대상 네트워크를 입력하세요.".to_owned());
    }

    let (addr_part, slash_prefix) = match dest.split_once('/') {
        Some((a, p)) => (a.trim(), Some(p.trim())),
        None => (dest, None),
    };
    let ip: IpAddr = addr_part
        .parse()
        .map_err(|_| format!("대상 주소 '{addr_part}'의 형식이 올바르지 않습니다."))?;

    let prefix_len = match (slash_prefix, mask.is_empty()) {
        (Some(_), false) => {
            return Err("대상에 /프리픽스를 썼다면 넷마스크 칸은 비워 두세요.".to_owned());
        }
        (Some(p), true) => parse_prefix_len(p, ip)?,
        (None, true) => max_prefix(ip),
        (None, false) => {
            if mask.chars().all(|c| c.is_ascii_digit()) {
                parse_prefix_len(mask, ip)?
            } else if ip.is_ipv4() {
                let m: Ipv4Addr = mask
                    .parse()
                    .map_err(|_| format!("넷마스크 '{mask}'의 형식이 올바르지 않습니다."))?;
                mask_to_prefix_v4(m)
                    .ok_or_else(|| format!("넷마스크 '{mask}'는 연속된 비트 마스크가 아닙니다."))?
            } else {
                return Err("IPv6는 프리픽스 길이(0~128)로 입력하세요.".to_owned());
            }
        }
    };

    Ok((network_addr(ip, prefix_len), prefix_len))
}

fn parse_prefix_len(text: &str, ip: IpAddr) -> Result<u8, String> {
    let max = max_prefix(ip);
    text.parse::<u8>()
        .ok()
        .filter(|n| *n <= max)
        .ok_or_else(|| format!("프리픽스 길이는 0~{max} 사이여야 합니다."))
}

/// 게이트웨이 입력을 해석한다. 비어 있거나 `0.0.0.0`/`::`이면 on-link(`None`).
pub fn parse_gateway(text: &str, dest: IpAddr) -> Result<Option<IpAddr>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let gw: IpAddr = text
        .parse()
        .map_err(|_| format!("게이트웨이 '{text}'의 형식이 올바르지 않습니다."))?;
    if gw.is_ipv4() != dest.is_ipv4() {
        return Err("대상과 게이트웨이의 IP 버전(IPv4/IPv6)이 다릅니다.".to_owned());
    }
    Ok((!gw.is_unspecified()).then_some(gw))
}

/// 게이트웨이와 같은 서브넷에 주소가 있는 인터페이스를 고른다 (가장 긴 프리픽스 우선).
pub fn pick_interface(gateway: IpAddr, interfaces: &[Interface]) -> Option<u32> {
    interfaces
        .iter()
        .flat_map(|iface| {
            iface
                .addrs
                .iter()
                .filter(move |(addr, plen)| {
                    addr.is_ipv4() == gateway.is_ipv4()
                        && network_addr(*addr, *plen) == network_addr(gateway, *plen)
                })
                .map(move |(_, plen)| (iface.connected, *plen, iface.index))
        })
        .max_by_key(|(connected, plen, _)| (*connected, *plen))
        .map(|(_, _, index)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn mask_prefix_roundtrip() {
        for n in 0..=32u8 {
            assert_eq!(mask_to_prefix_v4(prefix_to_mask_v4(n)), Some(n));
        }
        assert_eq!(prefix_to_mask_v4(24), Ipv4Addr::new(255, 255, 255, 0));
        assert_eq!(mask_to_prefix_v4(Ipv4Addr::new(255, 0, 255, 0)), None);
        assert_eq!(mask_to_prefix_v4(Ipv4Addr::new(0, 255, 0, 0)), None);
    }

    #[test]
    fn parse_destination_forms() {
        assert_eq!(parse_destination("10.0.0.0/8", ""), Ok((v4("10.0.0.0"), 8)));
        assert_eq!(
            parse_destination("10.0.0.0", "255.0.0.0"),
            Ok((v4("10.0.0.0"), 8))
        );
        assert_eq!(
            parse_destination(" 10.0.0.0 ", "8"),
            Ok((v4("10.0.0.0"), 8))
        );
        // 마스크 생략 = 호스트 라우트
        assert_eq!(parse_destination("10.1.2.3", ""), Ok((v4("10.1.2.3"), 32)));
        // 호스트 비트는 잘라낸다
        assert_eq!(
            parse_destination("192.168.5.77/24", ""),
            Ok((v4("192.168.5.0"), 24))
        );
        assert_eq!(parse_destination("0.0.0.0/0", ""), Ok((v4("0.0.0.0"), 0)));
        assert_eq!(
            parse_destination("2001:db8::1/32", ""),
            Ok((v4("2001:db8::"), 32))
        );
        assert_eq!(parse_destination("fd00::", "64"), Ok((v4("fd00::"), 64)));
    }

    #[test]
    fn parse_destination_errors() {
        assert!(parse_destination("", "").is_err());
        assert!(parse_destination("10.0.0", "").is_err());
        assert!(parse_destination("10.0.0.0/33", "").is_err());
        assert!(parse_destination("10.0.0.0", "255.0.255.0").is_err());
        assert!(parse_destination("10.0.0.0/8", "255.0.0.0").is_err());
        assert!(parse_destination("fd00::", "ffff::").is_err());
        assert!(parse_destination("fd00::/129", "").is_err());
    }

    #[test]
    fn parse_gateway_forms() {
        let dest = v4("10.0.0.0");
        assert_eq!(parse_gateway("", dest), Ok(None));
        assert_eq!(parse_gateway("0.0.0.0", dest), Ok(None));
        assert_eq!(
            parse_gateway("192.168.0.1", dest),
            Ok(Some(v4("192.168.0.1")))
        );
        assert!(parse_gateway("fe80::1", dest).is_err());
        assert!(parse_gateway("abc", dest).is_err());
    }

    #[test]
    fn pick_interface_prefers_longest_connected_prefix() {
        let iface = |index, addr: &str, plen, connected| Interface {
            index,
            connected,
            addrs: vec![(v4(addr), plen)],
            ..Default::default()
        };
        let list = [
            iface(1, "127.0.0.1", 8, true),
            iface(5, "192.168.0.10", 16, true),
            iface(7, "192.168.1.10", 24, true),
            iface(9, "192.168.1.20", 24, false),
        ];
        assert_eq!(pick_interface(v4("192.168.1.1"), &list), Some(7));
        assert_eq!(pick_interface(v4("192.168.200.1"), &list), Some(5));
        assert_eq!(pick_interface(v4("8.8.8.8"), &list), None);
    }

    /// 실제 시스템 라우팅 테이블을 읽는 스모크 테스트 (읽기 전용, 관리자 권한 불필요).
    #[test]
    fn reads_live_routing_table() {
        let routes = list_routes().expect("list_routes");
        let loopback = routes
            .iter()
            .find(|r| r.dest == v4("127.0.0.0") && r.prefix_len == 8)
            .expect("127.0.0.0/8 라우트가 있어야 함");
        assert!(loopback.is_on_link());

        let interfaces = list_interfaces().expect("list_interfaces");
        let lo = interfaces
            .iter()
            .find(|i| i.index == loopback.if_index)
            .expect("루프백 인터페이스");
        assert!(lo.addrs.iter().any(|(a, _)| *a == v4("127.0.0.1")));
        assert!(lo.metric_v4.is_some());
        assert!(!lo.alias.is_empty());
    }
}
