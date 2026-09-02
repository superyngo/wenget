# Repo 資源（Packages）篩選規則總覽

> **本文件為單一事實來源（Single Source of Truth）。**
> 未來新增或修改任何資源分析／篩選規則時，須先更新本文件，再修改對應程式碼。
> 每條規則均以 `檔案:函式`（含行號僅供參考，以函式名為準）標註實作位置。

分析流程分為兩大階段，另有數個輔助分類機制：

| 階段 | 內容 | 主要程式位置 |
|------|------|-------------|
| **階段一** | Release assets → 各平台分桶（platform buckets） | `src/core/platform.rs` — `BinarySelector::extract_platforms` → `score_parsed` |
| **階段二** | 目前平台 → 選定要安裝的平台分桶 | `src/core/platform.rs` — `possible_identifiers` / `find_best_match` / `fallback_identifiers` / `match_override` |
| **階段三** | 解壓後檔案 → 選定可執行檔 | `src/installer/extractor.rs` — `find_executable_candidates` |
| **輔助** | Release 取得、變體（variant）抽取、命令名正規化、glob 比對等 | 見第 5 節 |

> ⚠️ **實際運行路徑**：階段一的評分邏輯由 `score_parsed`（parse-once 版本）執行；
> `score_asset` / `select_for_platform` / `select_all_for_platform` 為演算法相同的
> `#[allow(dead_code)]` 相容版本（測試與向後相容用）。**修改規則時必須同步修改兩者**，
> 或以 `score_parsed` 為準。

---

## 0. 前置篩選：Release 層級

| # | 規則 | 實作位置 |
|---|------|---------|
| 0.1 | 只取 `/releases/latest`，**自動排除 draft 與 prerelease**（GitHub API 行為） | `src/providers/github.rs` — `GitHubProvider::fetch_latest_release` |
| 0.2 | 指定版本時，tag 依序嘗試「加 `v` 前綴」與「去 `v` 前綴」兩種形式 | `src/providers/github.rs` — `fetch_release_by_tag` |
| 0.3 | Release 無任何 assets → 直接報錯 | `src/providers/github.rs` — `fetch_package` / `fetch_package_by_version` |
| 0.4 | 階段一結束後平台 map 為空 → 直接報錯（無任何平台可安裝） | 同上 |

---

## 1. 階段一：Assets → 平台分桶

進入點：`BinarySelector::extract_platforms`（`src/core/platform.rs`）。
每個 asset 對 **11 個測試平台** 逐一評分（Windows x86_64/i686/aarch64、Linux x86_64/i686/aarch64/armv7、macOS x86_64/aarch64、FreeBSD x86_64/aarch64），
通過者放入 `platform_id → Vec<asset>` map；platform_id 格式為 `{os}-{arch}` 或 `{os}-{arch}-{compiler}`。

### 1.1 汰除規則（Gates，**依序執行**，任一命中即淘汰該 asset）

實作：`BinarySelector::score_parsed`（與 `score_asset` 相同）。

| 順序 | 規則 | 實作位置 |
|------|------|---------|
| G1 | **檔名黑名單**：檔名包含以下任一子字串即淘汰：`source`、`.deb`、`.rpm`、`.apk`、`.dmg`、`.pkg`、`.msi`、`.sha256`、`.sha512`、`.asc`、`.sig`、`checksums`、`checksum`、`.txt`、`.md` | `BinarySelector::should_exclude` |
| G2 | **不支援架構**：檔名包含 `UNSUPPORTED_ARCHS` 常數任一關鍵字即淘汰：s390x/s390、ppc64/ppc64le/ppc/powerpc/powerpc64/powerpc64le、riscv64/riscv32/riscv、mips/mips64/mipsel/mips64el、sparc64/sparc、alpha、sh4、hppa、ia64、loong64/loongarch64 | `ParsedAsset::contains_unsupported_arch` + `UNSUPPORTED_ARCHS` 常數 |
| G3 | **副檔名不支援**：`FileExtension::from_filename` 判為 `Unsupported` 即淘汰（支援清單見 1.3） | `FileExtension::from_filename` |
| G4 | **OS 必須匹配**：偵測不到 OS，或偵測到的 OS ≠ 目標平台 OS → 淘汰 | `score_parsed` OS matching |
| G5 | **架構明確不符**：偵測到明確架構且 ≠ 目標架構 → 淘汰 | `score_parsed` arch matching |
| G6 | **未偵測到架構時**：(a) 檔名含「未知架構樣式」（word-boundary 比對 `powerpc/ppc/riscv/mips/sparc/s390/alpha/sh4/hppa/ia64/loong`）→ 淘汰；(b) OS 無預設架構（FreeBSD）→ 淘汰 | `ParsedAsset::contains_unknown_arch_pattern` + `Os::default_arch` |

### 1.2 評分規則（加分制，順序無關，總分越高越優先）

| 項目 | 分數 | 說明 |
|------|------|------|
| OS 匹配（必要條件） | +100 | 見 G4 |
| 架構明確匹配 | +50 | |
| 架構未標明、以 OS 預設架構匹配 | +25 | Windows/Linux 預設 x86_64；macOS 預設 aarch64；FreeBSD 無預設 |
| Compiler/libc 優先度 | +priority×10 | Linux：musl(3) > gnu(2) > msvc(1)；Windows：msvc(3) > gnu(2) > musl(1)；macOS/FreeBSD 一律 1。見 `Compiler::priority` |
| 檔案格式偏好 | +2～+5 | `.tar.gz/.tgz`(5) > `.tar.xz`(4) > `.zip`/`.tar.bz2`(3) > `.7z`/`.exe`(2) > 無壓縮裸執行檔(1)。見 `FileExtension::format_score` |

### 1.3 檔名解析子規則（`ParsedAsset::from_filename`）

#### 副檔名偵測（`FileExtension::from_filename`，依序）
`.exe` → `.zip` → `.tar.gz`/`.tgz` → `.tar.xz` → `.tar.bz2` → `.7z` → 裸執行檔判定 → `Unsupported`。

**裸執行檔判定**（`is_likely_binary_without_extension`，依序）：
1. 排除非二進位副檔名：`.md .txt .rst .html .htm .json .yaml .yml .toml .xml .sha256 .sha512 .sig .asc .pub .pem .deb .rpm .apk .dmg .pkg .msi .appimage`
2. 排除含 `source` / `src` 的檔名
3. 檔名含平台關鍵字（windows/win64/win32/linux/darwin/macos/osx/mac/freebsd/x86_64/amd64/x64/aarch64/arm64/armv7/i686/x86/i386）→ **視為裸執行檔**
4. 無副檔名且檔名不含 readme/license/copying/changelog/authors/news/todo/makefile/dockerfile/vagrantfile/gemfile/rakefile → 視為裸執行檔

> 注意：此清單與 G1（`should_exclude`）、階段三的 `is_excluded_file` 是**三份獨立清單，服務不同階段，不可合併**。

#### OS 偵測（`ParsedAsset::detect_os`，依序）
1. 明確 OS 關鍵字，**檢查順序固定為 [MacOS, FreeBSD, Linux, Windows]**（因 "darwin" 含子字串 "win"，macOS 必須先於 Windows）：
   - Windows：`windows win64 win32 pc-windows win`
   - Linux：`linux unknown-linux`
   - macOS：`darwin macos apple osx mac`
   - FreeBSD：`freebsd`
2. Linux 發行版名稱推斷 → Linux：`ubuntu debian fedora centos alpine opensuse suse gentoo manjaro archlinux`
3. Arch Linux 命名慣例 → Linux：檔名含 `_arch-`/`-arch-` 或以 `_arch`/`-arch` 結尾
4. `.exe` 副檔名 → 推斷為 Windows
5. `.tar.gz`/`.tar.xz`/`.tar.bz2`/裸執行檔 **且** 檔名含架構關鍵字（x86_64/x64/amd64/aarch64/arm64/armv7/armhf/i686/i386/386）→ 推斷為 Linux
6. 皆無 → OS 未知（將被 G4 淘汰）

#### 架構偵測（`ParsedAsset::detect_arch`，依序）
1. **`x86` 特例優先**（檔名含 `x86` 但不含 `x86_64`）：macOS → x86_64（32-bit Mac 已淘汰）；其他/未知 OS → i686。見 `Arch::resolve_x86_keyword`
2. 依序比對 [X86_64, Aarch64, Armv7, I686] 的關鍵字（跳過 `x86`）：
   - X86_64：`x86_64 x64 amd64`
   - Aarch64：`aarch64 arm64`
   - Armv7：`armv7 armhf armv6 arm`
   - I686：`i686 x86 i386 386 win32`

#### Compiler/libc 偵測（`ParsedAsset::detect_compiler`，依序 [Musl, Msvc, Gnu]）
- Musl：`musl`；Msvc：`msvc`；Gnu：`gnu glibc`

---

## 2. 階段二：目前平台 → 選定平台分桶

### 2.1 精確匹配優先序（`Platform::possible_identifiers`）

依 **runtime libc 偵測**（`LibcType::detect`：先檢查 `/lib/ld-musl-*` 動態連結器，再 fallback 到 `ldd --version` 輸出含 "musl"，否則 Glibc）產生候選 id 順序：

| 系統 | 優先序 |
|------|--------|
| Linux (musl，如 Alpine) | `{base}-musl` > `{base}` > `{base}-gnu` |
| Linux (glibc / 未知) | `{base}-gnu` > `{base}` > `{base}-musl` |
| Windows | `{base}-msvc` > `{base}` > `{base}-gnu` |
| macOS / FreeBSD | 僅 `{base}` |

### 2.2 匹配流程（`Platform::find_best_match`）

1. **Phase 1 精確匹配**：依 2.1 順序查 platform map，命中即得分 `1000 - 優先序索引`。
2. **Phase 2 fallback**（僅在 Phase 1 全部落空時執行，`fallback_identifiers`）：

| 目前平台 | Fallback 目標 | 類型 | 分數 | 需使用者確認 |
|----------|--------------|------|------|:---:|
| Linux x86_64 | `linux-i686[-musl/-gnu]` | Arch32On64 | 300 | ✅ |
| macOS aarch64 | `macos-x86_64`（Rosetta 2） | X64OnArm | 200 | ✅ |
| Windows x86_64 | `windows-i686[-msvc/-gnu]` | Arch32On64 | 300 | ✅ |
| Windows aarch64 | `windows-x86_64[-msvc]`、`windows-i686` | X64OnArm | 200 | ✅ |
| （通用）musl 頂替 gnu | — | MuslOnGnu | 500 | ❌ |
| （通用）gnu 頂替 musl | — | GnuOnMusl | 400 | ✅ |
| （通用）Windows 編譯器變體 | — | WindowsCompilerVariant | 450 | ❌ |

需否確認見 `FallbackType::requires_confirmation`。最終依分數由高至低排序。

### 2.3 使用者覆寫（`Platform::match_override`，`-p/--platform` 旗標或 `preferred_platform` 設定）

1. 覆寫字串與 platform map 的 key **完全相同** → 直接採用（分數 1000）。
2. 否則以 `ParsedAsset::from_filename` 寬鬆解析（支援 Rust target triple 如 `aarch64-unknown-linux-musl`、寬鬆格式如 `windows-x64`）：
   - OS+arch 皆解析成功 → 走 `find_best_match`
   - 只有 OS → 用 `Os::default_arch` 補架構（FreeBSD 無預設則失敗）
3. 若覆寫字串指定了 compiler/libc（如 musl），且該變體存在 → **提升至第一位**（分數 2000）。

---

## 3. 階段三：解壓後檔案 → 可執行檔選定

進入點：`find_executable_candidates`（`src/installer/extractor.rs`）；
`find_executable` 取排序後第一名。

### 3.1 汰除規則（Gates，**依序執行**，任一命中即跳過該檔案）

| 順序 | 規則 | 實作位置 |
|------|------|---------|
| G1 | **排除文件/設定檔**（`is_excluded_file`）：<br>(a) 文件副檔名：`.md .txt .rst .html .htm .pdf .doc .docx` 及 man page `.1`～`.8`<br>(b) 檔名含：`license licence copying unlicense notice readme changelog changes history authors contributors credits thanks todo news`<br>(c) 設定檔副檔名：`.yml .yaml .toml .json .xml .ini .cfg .conf`<br>(d) 位於 `complete`/`completion` 目錄下的 `.fish .bash .zsh .ps1` 補全檔<br>(e) 位於補全目錄下且以 `_` 開頭的檔案（如 zsh 的 `_rg`） | `is_excluded_file` |
| G2 | **可執行檔形態檢查**（`could_be_executable`）：<br>Windows：必須以 `.exe` 結尾。<br>Unix：位於 `bin/` 目錄 **或** 檔名無副檔名 **或** 為 `.sh` 腳本，三者其一 | `could_be_executable` |
| G3 | **排除測試/除錯檔**：檔名（僅檔名，不含路徑）含 `test`、`debug`、`bench`、`example` 任一者 → 淘汰 | `find_executable_candidates` 內 |

### 3.2 評分規則（加分制；比對名稱時先去除 `.exe`）

| 規則 | 分數 | 說明 |
|------|------|------|
| Rule 0：具執行權限（Unix，mode & 0o111） | +35 | `has_executable_permission` |
| Rule 0b：magic bytes 為原生二進位（ELF `\x7fELF` / PE `MZ` / Mach-O 各 magic） | +60 | `detect_executable_type`（最強訊號） |
| Rule 0b'：shebang 腳本（`#!`，辨識 python/bash/sh/node/ruby/perl） | +30 | `detect_script_type`（與 +60 互斥，二進位優先） |
| Rule 1：檔名 == 套件名（完全相同） | +100 | |
| Rule 2：檔名與套件名互為包含（部分匹配） | +50 | |
| Rule 3：疑似縮寫（如 ripgrep → rg：各分段首字母，或為套件名前綴） | +40 | `is_likely_abbreviation` |
| Rule 4：位於 `bin/` 目錄 | +40 | |
| Rule 5：位於 `target/release/`（Rust 專案） | +25 | |
| Rule 6：目錄深度淺：深度 ≤1 | +20；深度 ≤2 | +10 | |
| Rule 7：簡單檔名（不含 `-` 或 `_`） | +5 | |

**入選門檻**：`score > 0` **或** 具執行權限（Unix）。最終依分數由高至低排序。

### 3.3 裸執行檔判定（安裝時，`is_standalone_executable`）

決定下載物直接複製或走解壓流程：
- Windows：`.exe` → 裸執行檔
- Unix：`.AppImage` → 裸執行檔；檔名不含任何壓縮副檔名（`.zip .tar .gz .xz .bz2 .7z .rar .tbz .tgz`）→ 視為裸執行檔
- 支援的壓縮格式（`extract_archive` 依序判斷）：`.tar.gz/.tgz` → `.tar.xz` → `.tar.bz2/.tbz` → `.zip` → `.7z`，其餘報錯

---

## 4. 變體（Variant）抽取規則

實作：`extract_variant_from_asset`（`src/core/manifest.rs`）。用於辨識同 repo 多變體（如 `bun` / `bun-baseline` / `bun-profile`）。**處理順序固定**：

1. 去除副檔名：`.zip .tar.gz .tar.xz .exe .7z .tgz`
2. 去除 repo 名前綴（不分大小寫），再去除前導 `-`/`_`
3. 底線正規化為連字號
4. 去除版本號分段（以 `-` 分段後，過濾「可含 `v` 前綴、以數字開頭且含 `.`、全為數字與點」的分段）
5. 去除 `unknown` 關鍵字（Rust target triple 常見）
6. **依特定性由高至低**移除平台樣式（先 OS-arch-compiler 組合 → OS-arch 組合 → 純 arch → 純 OS → 其他：win32/win64/win/musl/gnu/msvc/pc），每個 pattern 依序嘗試 `-pattern`、`_pattern`、裸 pattern 三種形式
7. 清理連續 `--`/`__`，修剪首尾 `-`/`_`
8. 剩餘為空 → 無變體（`None`）；否則即為變體名

安裝鍵格式（`generate_installed_key`）：無變體 → `{repo_name}`；有變體 → `{repo_name}::{variant}`。

---

## 5. 其他分類比對機制

### 5.1 命令名正規化（`normalize_command_name`，`src/installer/extractor.rs`）

去除平台後綴以產生乾淨命令名（如 `cate-windows-x86_64.exe` → `cate`）：
1. 檔名含任一平台關鍵字（`windows linux darwin macos freebsd netbsd openbsd x86_64 aarch64 arm64 armv7 i686 x64 x86 pc unknown gnu musl msvc`，不分大小寫）→ 從**第一個** `-` 或 `_` 處截斷
2. 一律去除結尾 `.exe`

> 注意副作用：不含平台關鍵字的名稱（如 `git-lfs.exe`）保留連字號不截斷。

### 5.2 套件輸入解析與 glob 比對（`src/package_resolver.rs`）

- **輸入分類**（`PackageInput::parse`）：以 `http://`、`https://`、`github.com/` 開頭 → DirectUrl（並經 `normalize_github_url` 正規化：http→https、補 https、去尾斜線、去 `.git`）；否則 → CacheName。
- **Cache 名稱解析**（`resolve_from_cache`，依序）：
  1. `repo::variant` 格式先取 `::` 前的 base name
  2. 含 `*` → glob 比對（`glob_match`：支援任意位置多個 `*` 萬用字元，錨定頭尾）；不含 → 完全相符
  3. Cache 未命中且非 glob → 查 `installed.json` 中 DirectRepo 來源的已安裝套件，改走 URL 解析
  4. 皆未命中 → 依情境報錯

### 5.3 安裝後命令名衝突解決（僅供參照，非資源篩選）

`resolve_command_name`（`src/commands/add.rs`）：自訂名 → 直接用或加數字後綴；變體 → 附加變體後綴（已含則不重複），衝突時再加數字後綴 `-1`～`-99`。`--no-suffix` 旗標跳過變體後綴。細節見該函式，不在本文件範圍內展開。

---

## 6. 修改規則時的檢查清單

- [ ] 先更新本文件對應章節，再改程式碼
- [ ] 階段一評分改動：`score_parsed` 與 `score_asset` **兩處同步**
- [ ] 三份排除清單（1.3 裸執行檔排除、1.1 G1、3.1 G1）**各自獨立**，確認改到正確的一份
- [ ] 順序敏感規則（1.3 OS 偵測順序、4 平台樣式移除順序、3.1 gates 順序）改動時，確認既有測試涵蓋順序行為
- [ ] 執行 `cargo test`（platform、extractor、manifest 模組均有行為測試）
