# RouteWorker (구 DHRouteView) — 에이전트/세션 메모

Windows 라우팅 테이블을 GUI로 조회·설정하는 Rust 프로그램. 이 파일은 세션 간 인수인계용 메모다.
작업을 마칠 때마다 아래 "세션 기록"과 "다음 할 일"을 갱신할 것.

## 한눈에 보기

- 언어/툴체인: Rust (edition 2024), `stable-x86_64-pc-windows-msvc`
- GUI: `eframe`/`egui` 0.37 + `egui_extras`(테이블). 렌더러는 기본값(wgpu).
- Windows API: `windows` 0.62 (IP Helper), `windows-registry` 0.100
- UI 문구: 한국어. 한글 글꼴은 eframe의 `system_fonts` 기능(시스템 글꼴 자동 대체)에 의존.

## 빌드 / 실행 / 테스트

```powershell
cargo build              # 디버그 (콘솔 창 있음)
cargo build --release    # 릴리스 (콘솔 창 없음) → target\release\routeworker.exe
cargo test               # 단위 테스트 + 읽기 전용 스모크 테스트 (관리자 권한 불필요)
```

라우트 변경(추가/편집/삭제)은 관리자 권한이 필요하다. 일반 권한으로 실행하면 읽기 전용이고,
앱 안의 "관리자 권한으로 다시 시작" 버튼이 UAC로 재실행한다.

## 구조

| 파일 | 역할 |
| --- | --- |
| `src/main.rs` | 진입점, 창 옵션 (창 아이콘) |
| `src/app.rs` | egui UI 전체 (도구 모음, 테이블, 추가/편집·삭제 확인·정보 대화 상자, 상태 표시줄) |
| `build.rs` | exe 아이콘 + 버전 정보 리소스 (`winresource`) |
| `assets/` | 앱 아이콘(.ico/.png/.svg), 회사 로고 PNG — 원본은 DHPLab 브랜드 폴더에서 받음 |
| `src/route.rs` | IP Helper API 래퍼 + 주소 파싱/검증 순수 함수 + 테스트 |
| `src/persist.rs` | 영구 라우트 (레지스트리) |
| `src/selection.rs` | 다중 선택 상태 (클릭 / Ctrl+클릭 / Shift+클릭 규칙) + 테스트 |
| `src/elevate.rs` | 관리자 권한 확인, UAC 재실행 |
| `tools/capture.ps1` | 앱 창 캡처 + 합성 클릭 (UI 확인용, 아래 참고) |

## 설계 결정 (바꾸기 전에 이유 확인)

- **`route.exe`를 호출/파싱하지 않는다.** `GetIpForwardTable2` / `CreateIpForwardEntry2` /
  `SetIpForwardEntry2` / `DeleteIpForwardEntry2`를 직접 쓴다 (로캘 무관, 오류 코드 명확).
- **메트릭**: API의 `Metric`은 라우트 메트릭이고, `route print`에 보이는 값은
  라우트 메트릭 + 인터페이스 메트릭이다. 테이블에는 합산 값을 보여 주고, 폼에서는 라우트 메트릭을 입력받는다.
- **영구 라우트**는 `route -p`와 같은 저장소
  (`HKLM\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\PersistentRoutes`,
  값 이름 `대상,마스크,게이트웨이,메트릭`, 데이터는 빈 REG_SZ)를 쓴다.
  인터페이스 정보가 저장되지 않으므로 IPv4 + 게이트웨이 있는 라우트만 허용한다.
- **편집**: 키(대상/게이트웨이/인터페이스)가 같으면 메트릭만 `SetIpForwardEntry2`로 바꾸고,
  키가 바뀌면 "새 라우트 추가 → 성공 시 기존 라우트 삭제" 순서로 처리한다.
- **권한**: 매니페스트에서 항상 관리자 권한을 요구하지 않는다(조회만 할 때 UAC가 뜨지 않게, `cargo run`도 가능하게).
- **인터페이스 자동 선택**: 게이트웨이와 같은 서브넷 주소를 가진 인터페이스 중 가장 긴 프리픽스(연결된 것 우선).
  on-link 라우트(게이트웨이 없음)는 인터페이스를 직접 골라야 한다.
- **연결 끊긴 인터페이스의 라우트**는 기본적으로 숨긴다 (`GetIpForwardTable2`는 모두 돌려주지만
  `route print`는 숨기므로 그에 맞춤). 도구 모음의 체크박스로 볼 수 있다.
- **게이트웨이 없는 라우트는 "On-link"로 표기한다.** 처음에는 "연결됨 (on-link)"이라 썼는데,
  연결이 끊긴 인터페이스의 행에서도 "연결됨"으로 읽혀 사용자가 혼동했다. on-link는 링크 상태와 무관하므로
  "연결" 계열 단어를 쓰지 말 것. 연결 끊긴 인터페이스의 행은 전체를 흐리게 표시한다.
- **다중 선택**: 탐색기와 같은 규칙(클릭=단일, Ctrl=토글, Shift=범위, Ctrl+Shift=범위 추가),
  Ctrl+A 전체 선택, Esc 해제, Delete 삭제. 필터에 가려 보이지 않는 행은 매 프레임 선택에서 빠진다
  (보이지 않는 라우트를 지우지 않기 위함). 편집은 정확히 1개 선택일 때만 가능.
  일괄 삭제는 일부가 실패해도 나머지를 계속 진행하고 결과를 "N개 삭제, M개 실패"로 알린다.
- **입력 정규화**: 대상에 호스트 비트가 섞여 있으면 네트워크 주소로 잘라내고, 폼 아래 "적용 내용"에 미리 보여 준다.

## eframe/egui 0.37 주의점 (이전 버전과 다름)

- `App::update(ctx, frame)`가 아니라 `App::ui(&mut self, ui: &mut egui::Ui, frame)`를 구현한다.
- 패널은 `egui::Panel::top("id").show(ui, …)` / `egui::CentralPanel::default().show(ui, …)` — `ctx`가 아니라 `ui`를 넘긴다.
- 모달은 `egui::Modal::new(id).show(ctx, …)`, 닫힘 판정은 `ModalResponse::should_close()`.
- 테이블 행을 클릭 가능하게 하려면 `TableBuilder::sense(Sense::click())` +
  `ui.style_mut().interaction.selectable_labels = false` (라벨이 클릭을 가로채지 않게).
- API가 헷갈리면 `~\.cargo\registry\src\index.crates.io-*\egui-0.37.0\src`를 직접 확인할 것.

## 세션 기록

### 2026-11-15 (첫 세션)

- 사용자 요청: "Windows에서 route 설정 할 수 있는 프로그램을 GUI로, 소스는 Rust로".
  메시지가 "…진행 했으면 하고"로 끝나 추가 요구가 더 있을 수 있음.
- 사용자 요청: 세션 메모는 이 `AGENTS.md`에, 세션 기억은 메모리에도 따로 남길 것.
- 빈 폴더에서 `cargo init`, 위 구조대로 전체 구현.
- 검증한 것 (일반 권한 세션에서):
  - `cargo test` 9개 통과, `cargo clippy` 경고 없음, `cargo build --release` 성공 (exe 약 15MB).
  - 앱을 실제로 띄워 창을 캡처해 확인: 한글 렌더링 정상, IPv4 16개 행과 메트릭 값이 `route print -4`와 일치,
    행 클릭 선택, 우클릭 메뉴, 정렬 화살표, 추가 대화 상자(미리 보기: 호스트 비트 정리 + 인터페이스 자동 선택),
    삭제 확인 대화 상자(기본 경로 경고).
  - 권한 없이 "추가"를 누르면 `CreateIpForwardEntry2`가 오류 5로 거부되고 폼에 한국어 오류가 표시됨.
- **검증하지 못한 것** (이 세션은 관리자 권한이 아니었음):
  - 관리자 권한에서의 실제 추가/편집/삭제 성공 경로, 메트릭만 바꾸는 `SetIpForwardEntry2` 경로.
  - 영구 라우트 레지스트리 쓰기/삭제, 그리고 그 항목이 재부팅 후 실제로 적용되는지.
  - "영구 라우트" 탭에 항목이 있을 때의 화면, "관리자 권한으로 다시 시작" 버튼(UAC).
  - 폼에 키보드로 직접 입력하는 동작(합성 키 입력이 winit에 전달되지 않아 값은 코드로 채워 확인함).
- 도중에 고친 것: 정렬 화살표 글리프(⏶⏷ → ⬆⬇, 기본 글꼴에 없음), 연결 끊긴 인터페이스의 라우트가
  `route print`와 달리 모두 보이던 문제(기본 숨김 + "연결 끊긴 인터페이스 포함" 체크박스 추가).

### 2026-11-15 (같은 날 후속 요청)

- 사용자 피드백 1: "연결이 끊긴 라인도 연결됨(on-link)로 표시됨" → 표기를 "On-link"로 바꾸고 연결 끊긴 행을 흐리게 함.
- 사용자 피드백 2: 하나씩 선택 + Shift/Ctrl로 여러 개 선택해 지우는 기능 요청 → `src/selection.rs` 추가,
  도구 모음 "삭제 (N)", 우클릭 메뉴 "선택한 N개 삭제 / 명령으로 복사", 일괄 삭제 확인 대화 상자, 단축키 추가.
- 검증: `cargo test` 16개 통과(선택 규칙 7개 포함), clippy 경고 없음, 릴리스 빌드 성공.
  창 캡처로 3개 다중 선택 강조, "삭제 (3)", 3개 삭제 확인 대화 상자(기본 경로·자동 라우트 경고), "On-link" 표기 확인.
- 에이전트가 직접 검증하지 못했던 것: 실제 키보드의 Ctrl/Shift 클릭(합성 입력으로는 수정 키를 전달할 수 없어
  확인할 때 `ctrl`을 코드로 강제함), Delete/Ctrl+A/Esc 단축키, 관리자 권한에서의 실제 삭제.
- **사용자 확인 완료 (2026-11-15, 사용자가 직접 실행해 보고 알려 줌)**:
  - 실제 키보드로 Ctrl/Shift+클릭, Delete, Ctrl+A, Esc 동작.
  - 관리자 권한에서 단일 삭제, 다중 삭제, 일괄 삭제.
  - 관리자 권한에서 라우트 추가와 편집 (사용자 발언: "추가 편집도 확인함". 편집 중 어떤 경우 —
    메트릭만 변경 / 대상·게이트웨이 변경 — 를 해 봤는지는 말하지 않았음).
- 아직 누구도 확인했다고 한 적 없는 것: 영구 라우트 저장/삭제와 재부팅 후 적용,
  "영구 라우트" 탭에 항목이 있을 때의 화면.

- 세션 종료: 사용자가 계속 사용해 보면서 부족한 점이나 문제가 생기면 알려 주기로 함. 진행 중인 작업 없음.

### 2026-11-16 (GitHub 업로드)

- 사용자 요청: "정리해서 github에 올려줘".
- `README.md` 추가, 브랜치 `master` → `main`, 첫 커밋.
- GitHub CLI(gh)를 winget으로 설치하고 사용자가 브라우저로 로그인함 (계정 `pdh0085`).
- 저장소: https://github.com/pdh0085/DHRouteView — 처음에는 **Private**(사용자 선택)로 만들었음, 기본 브랜치 `main`.
  2026-11-17 확인 시 **Public**으로 바뀌어 있음 (언제 바꿨는지는 기록 없음).
- 이어서 사용자 요청으로 MIT 라이선스 추가 (`LICENSE`, 저작권자 Donghyun Park, `Cargo.toml`의 `license`, README).
- 사용자 요청으로 GitHub 릴리스 `v0.1.0` 생성 (태그 v0.1.0 → main). 첨부: `DHRouteView.exe`(릴리스 빌드),
  `DHRouteView-v0.1.0-windows-x64.zip`(exe + LICENSE + README). 코드 서명 없음.
  다음 릴리스 때는 `Cargo.toml`의 `version`을 먼저 올릴 것. PowerShell 5.1에서 `gh --notes`에 따옴표가 든
  한국어 문자열을 넘기면 인자가 깨지므로 `--notes-file`(UTF-8, BOM 없음)을 쓴다.

### 2026-11-17 (블로그 원고, 클라우드 세션)

- 사용자 요청: 이 프로젝트를 소개하는 짧은 블로그 원고 정리. 원고는 저장소에 넣지 않음 (세션 임시 폴더에만 있음).
- 사용자가 알려 준 제작 배경 (저장소에 없던 정보): 원격 지원 업무가 많은데, 현장 작업자가 사내에서 개발한
  라우터 제품을 PC에 꽂아 두면 라우팅 테이블이 바뀌어 원격 지원이 불가능해짐. 작업자는 Windows 명령을 모름.
  예전에는 Skype, 지금은 Teams로 통화하며 명령어를 불러 줬음. 제품 종류는 여러 개라 원고에 나열하지 않기로 함.
- 저장소가 Public이 된 것을 확인해 위 기록을 고침. 원고에 저장소·릴리스 v0.1.0 링크를 넣음.
- **사용자 확인 완료 (2026-11-17)**: "관리자 권한으로 다시 시작" 버튼(UAC 재실행) 시험 완료.

### 2026-11-17 (블로그용 스크린샷)

- 사용자 요청: 블로그 글에 넣을 스크린샷 3장(`main.png`, `add-dialog.png`, `bulk-delete.png`)을
  `바탕 화면\dhrouteview-shots\`(이 PC의 바탕 화면은 OneDrive 동기화 폴더 아래에 있음)에.
  실제 라우트는 바꾸지 말 것, 임시 코드 변경은 원복하고 `git diff`가 빈 것까지 확인할 것,
  민감 정보는 가리지 말고 이미지별로 보고만 할 것.
- `main.png`는 수정 없는 빌드(일반 권한, 읽기 전용 배너 보임). 나머지 두 장은 `elevated = true`로 찍음.
- `capture.ps1`의 `L:` 클릭이 이번에는 전혀 먹지 않아(아래 참고) 대화 상자는 `App::new`에서 코드로 열었다:
  추가 = `RouteForm::new()`에 `198.51.100.7/24`와 이 PC의 Wi-Fi 게이트웨이(자동 선택이 보이도록)를 채워 열기,
  일괄 삭제 = 루프백 라우트 3개(127.x)를 선택한 뒤 `confirm_delete_selected()`.
  두 대화 상자 모두 "취소"를 눌러 닫았고(닫힌 화면과 상태 표시줄 "준비"로 확인), 라우트 수는 변하지 않았다.
- 원복 확인: `git diff`/`git status` 비어 있음, `src/app.rs` blob 해시가 HEAD와 같음, 디버그 exe도 원본 소스로 다시 빌드함.
- 이 세션은 관리자 권한이 아니었으므로 `elevated`를 강제해도 실제 API 호출은 거부되는 상태였다.
- **저장소가 Public이므로** 이 파일에는 로컬 사용자 경로, 이 PC의 IP·인터페이스 이름 같은 환경 정보를 적지 않는다.
- 이어서 (같은 세션):
  - 이 기록을 푸시하려다 거부됨 — 클라우드 세션이 `origin/main`에 먼저 커밋 2개를 올려 둔 상태였음.
    fast-forward 후 다시 얹어 푸시. **푸시 전에는 `git fetch`로 원격 상태를 확인할 것.**
  - 사용자 요청으로 이 저장소의 `claude/lucid-edison-1yuoam` 브랜치 삭제 (내용은 이미 `main`에 있었음).
  - 사용자 요청으로 스크린샷 3장을 블로그 저장소(`pdh0085.github.io`)의 같은 이름 브랜치
    `static/images/dhrouteview/`에 커밋·푸시. 이 저장소에는 넣지 않음.
  - **사용자 요청으로 커밋 메시지의 `Co-Authored-By` 줄을 전부 지움**: `git filter-branch --msg-filter`로 `main` 이력을
    다시 쓰고(작성자·날짜·파일 내용은 그대로) `main`과 태그 `v0.1.0`을 강제 푸시함. 이때 모든 커밋 해시가 바뀌었다.
    v0.1.0 릴리스와 첨부 파일은 그대로. 다른 곳의 옛 사본은 새 `main`을 다시 받아야 한다.
- **커밋 규칙**: 커밋 메시지에 `Co-Authored-By` 줄을 넣지 않는다 (사용자 지시). 작성자는 git 설정 그대로.

### 2026-11-22 (이름 변경, 저장소 이동)

- 사용자 요청: 프로젝트 이름을 **RouteWorker**로 바꾸고 원격 저장소를
  https://github.com/donghyunpark-dhplab/ROUTEWorker 로 옮김 (사용자가 미리 만들어 둔 빈 저장소).
- 바꾼 것: 크레이트 이름 `routeworker`(exe는 `routeworker.exe`), 창 제목 `APP_TITLE`, README 제목·빌드 경로,
  이 파일의 제목·현재 사용법. 위 세션 기록의 옛 이름·옛 저장소 주소는 기록이므로 그대로 둠.
- `origin`을 새 주소로 바꾸고 `main`과 태그 `v0.1.0`을 푸시함. GitHub 릴리스(첨부 exe/zip)는 git push로 옮겨지지 않는다.
- 로컬 폴더 이름은 아직 `DHRouteView` 그대로.
- 이 PC에서 저장소 폴더 소유자가 현재 Windows 계정과 달라 git이 "dubious ownership"으로 거부했다.
  이번에는 명령마다 `-c safe.directory=…`로 우회함 (전역 설정은 바꾸지 않음). `gh`도 PATH에 없었다.
- 사용자 결정: **옛 저장소 `pdh0085/DHRouteView`는 그대로 둔다** (보관 처리·안내 문구·삭제 모두 하지 않음).
- `gh`를 winget으로 다시 설치하고 사용자가 `donghyunpark-dhplab` 계정으로 로그인함.
  설치 직후 세션에서는 PATH가 반영되지 않아 `C:\Program Files\GitHub CLI\gh.exe` 전체 경로로 실행했다.
  기기 코드 로그인은 사용자가 바로 승인하지 않으면 만료되므로, 사용자가 직접 `gh auth login --web`을 실행하는 편이 낫다.
- 사용자 요청으로 새 저장소에 릴리스 **`v0.2.0`** 생성 (`Cargo.toml` version 0.2.0, 이름 변경 외 기능 변경 없음).
  첨부: `RouteWorker.exe`, `RouteWorker-v0.2.0-windows-x64.zip`(exe + LICENSE + README). 코드 서명 없음.
  설치 파일(MSI 등)은 만들지 않기로 함 — 단일 exe라 설치할 것이 없고, 현장 작업자는 받아서 바로 실행하는 편이 쉽다.
  v0.1.0은 새 저장소에 태그만 있고 릴리스 페이지는 옛 저장소에만 있다.
- 같은 날 사용자 요청으로 dhplab.com 제품 페이지용 스크린샷을 만들어 다른 Claude 세션("DHPLab", 회사 웹사이트 담당)에
  넘김 (그쪽 저장소 폴더에 저장). 공개용이므로 IPv4 화면만 사용 — IPv6 기본 경로의 next hop이 라우터 MAC 기반
  링크 로컬 주소라서. 1400px 폭을 위해 창 크기·`elevated`·폼 열기를 임시 코드로 바꿨다가 원복함.
  **주의: 원복할 때 `git checkout -- 파일`을 쓰지 말 것** — 커밋 안 한 진짜 변경까지 지워진다. 임시 줄만 지운다.

### 2026-11-22 (v0.2.1: 아이콘, 정보 화면)

- 사용자 요청: 로고·아이콘 변경과 About 추가. 디자인은 DHPLab 세션이 준비 — 후보 3개 중 사용자가 **A(fork)** 선택.
- 사용자 결정: 저작권 표기 **"Copyright (c) 2026 DHPLab Consulting"** (LICENSE도 변경), 앱 안 문의처는
  **웹사이트만** (이메일 넣지 않음). 퍼블리셔 표기 "DHPLab Consulting".
- 추가한 것:
  - `assets/routeworker.ico`: DHPLab이 준 PNG 16~256px를 PNG 압축 항목으로 묶어 직접 만듦.
    System.Drawing.Icon은 256px PNG 항목을 못 읽으니 WIC(`IconBitmapDecoder`)로 7개 크기를 확인할 것.
  - `build.rs`(`winresource`): exe 아이콘, ProductName/CompanyName/LegalCopyright 등 버전 정보.
  - 창 아이콘: `eframe::icon_data::from_png_bytes`(추가 의존성 없음).
  - 도구 모음 오른쪽 끝 "정보" 버튼 → 정보 대화 상자: 아이콘, 이름, 버전(`CARGO_PKG_VERSION`), 한 줄 설명,
    DHPLab 가로 로고(밝은 배경용, 2000px 원본을 640px로 축소만 함, 180px로 표시 — 브랜드 규칙: 최소 140px,
    여백은 마크 높이의 1/4 이상, 색·비율 변경 금지 — `dhplab-web\docs\BRAND.md`), 저작권, MIT, dhplab.com·GitHub 링크.
  - 제품 페이지 https://dhplab.com/routeworker/ 링크는 DHPLab 세션이 공개됐다고 알려 준 뒤에 넣기로 함 (아직 안 넣음).
- `Cargo.toml` version 0.2.1, 릴리스 v0.2.1 (첨부 이름 규칙은 v0.2.0과 같음).

## UI를 눈으로 확인하는 방법 (마우스/키보드 제어 없이)

`tools/capture.ps1`이 exe를 띄우고 `PrintWindow`로 창만 캡처한다. 실제 커서를 움직이지 않고
`PostMessage`로 클릭을 보낼 수 있다 (좌표는 캡처 이미지 기준 픽셀).

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools\capture.ps1 `
  -Exe target\debug\routeworker.exe -Out C:\임시경로\shot.png -Clicks "L:100,250;R:100,340"
```

- `L:` 좌클릭, `R:` 우클릭, `D:` 더블클릭. `T:`(문자 입력)는 동작하지 않는다.
- 사용자가 실제 마우스를 그 창 위에 올려 두면 클릭 위치가 어긋날 수 있다.
- **반대로 실제 커서가 창 밖에 있으면 `L:` 클릭이 아예 먹지 않을 수 있다** (2026-11-17, 호버만 되고 클릭 무반응).
  원인 추정: 스크립트가 이동→150ms→누름→60ms→뗌으로 쉬어 가며 보내는 사이에 winit이 실제 커서 위치 기준의
  `WM_MOUSELEAVE`/이동을 받아 egui가 포인터를 잃고, 그 뒤 온 버튼 메시지는 버려진다. 한 번 이렇게 되면
  그 인스턴스는 이후 클릭도 계속 무시했으므로 새로 띄울 것.
  이동·누름·뗌을 **쉬지 않고 연달아** `PostMessage`하면 동작했다 (egui가 같은 프레임의 터치 탭처럼 처리).
  `capture.ps1`에는 아직 반영하지 않았다.
- 클릭에 기대지 않는 가장 확실한 방법: 원하는 대화 상자/선택 상태를 `App::new`에서 코드로 만들고 캡처한 뒤 원복.
- 관리자 전용 버튼/대화 상자를 보려면 `App::new`의 `elevated`를 잠깐 `true`로 바꿔 확인하고 **반드시 원복**한다
  (실제 API 호출은 권한이 없으면 거부되므로 시스템은 바뀌지 않는다).

## 다음 할 일 / 미해결

- ~~실제 키보드로 Ctrl/Shift+클릭, Delete, Ctrl+A, Esc 동작 확인~~ — 사용자 확인 완료 (2026-11-15).
- ~~관리자 권한에서 단일/다중/일괄 삭제~~ — 사용자 확인 완료 (2026-11-15).
- ~~관리자 권한에서 라우트 추가 → 편집~~ — 사용자 확인 완료 (2026-11-15).
- 영구 라우트 저장/삭제와 재부팅 후 적용 여부 확인
  (테스트용 대상은 문서용 대역 `198.51.100.0/24` 권장).
- ~~사용자의 첫 요청 문장이 끝나지 않았으므로 추가 요구 사항 확인~~ — 사용자가 "내 요구는 거의 완벽하게
  구현한 것 같다"고 함 (2026-11-15). "거의"에 해당하는 부족한 점이 무엇인지는 말하지 않았음.
- 후보 개선(요청받은 것은 아님): `tools/capture.ps1` 클릭을 "연달아 보내기" 방식으로 바꾸기, ~~exe 아이콘/버전 리소스~~(v0.2.1에서 함), 정보 화면에 제품 페이지 링크(공개 후), 라우트 목록 내보내기·가져오기,
  IPv6 영구 라우트(netsh 저장소), 릴리스 exe 크기 축소(`glow` 렌더러, `opt-level="s"`, LTO).
