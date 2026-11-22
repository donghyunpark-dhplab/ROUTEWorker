# RouteWorker

Windows 라우팅 테이블을 GUI로 조회하고 추가·편집·삭제하는 프로그램입니다. Rust + [egui](https://github.com/emilk/egui)로 만들었습니다.

`route.exe`를 호출하지 않고 IP Helper API(`GetIpForwardTable2`, `CreateIpForwardEntry2`,
`SetIpForwardEntry2`, `DeleteIpForwardEntry2`)를 직접 사용하므로 로캘과 무관하게 동작합니다.

## 기능

- IPv4 / IPv6 라우팅 테이블 조회, 열 정렬, 필터
- 라우트 추가 / 편집 / 삭제
  - 게이트웨이에 맞는 인터페이스 자동 선택
  - 대상 주소의 호스트 비트 자동 정리와 "적용 내용" 미리 보기
- 다중 선택 (클릭, Ctrl+클릭, Shift+클릭, Ctrl+A, Esc)과 일괄 삭제 (Delete)
- 영구 라우트 (`route -p`와 같은 레지스트리 저장소, IPv4 + 게이트웨이 라우트)
- 선택한 라우트를 `route` 명령으로 복사
- 연결 끊긴 인터페이스의 라우트는 기본으로 숨김 (`route print`와 동일), 체크박스로 표시 가능

표의 메트릭은 `route print`와 같이 라우트 메트릭 + 인터페이스 메트릭의 합이며,
추가/편집 폼에서는 라우트 메트릭을 입력합니다.

## 권한

조회는 일반 권한으로 가능합니다. 라우트를 변경하려면 관리자 권한이 필요하며,
앱의 "관리자 권한으로 다시 시작" 버튼으로 UAC를 거쳐 다시 실행할 수 있습니다.

## 빌드

Rust stable (MSVC 툴체인, edition 2024)이 필요합니다.

```powershell
cargo build --release    # target\release\routeworker.exe
cargo test               # 관리자 권한 불필요
```

## 구조

| 파일 | 역할 |
| --- | --- |
| `src/main.rs` | 진입점, 창 옵션 |
| `src/app.rs` | egui UI |
| `src/route.rs` | IP Helper API 래퍼, 주소 파싱/검증 |
| `src/persist.rs` | 영구 라우트 (레지스트리) |
| `src/selection.rs` | 다중 선택 규칙 |
| `src/elevate.rs` | 관리자 권한 확인, UAC 재실행 |
| `tools/capture.ps1` | 앱 창 캡처 (UI 확인용 개발 도구) |

설계 결정과 개발 메모는 [AGENTS.md](AGENTS.md)에 있습니다.

## 라이선스

[MIT](LICENSE) · Copyright (c) 2026 DHPLab Consulting · https://dhplab.com
