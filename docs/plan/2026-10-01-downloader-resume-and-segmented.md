# 下載器：重試續傳 + 切片多連線 + 跨次續傳
Status: Shipped (2026-10-01)

Commits `3c844fe`、`b415ac3`、`6535ca1`；實測結果見文末〈驗證結果〉。

## 目標

痛點（使用者確認）：單一大檔下載很慢，失敗後要從頭重下。

- 傳輸中途失敗，接著從已下載的位置繼續，不從 0 重來（同一次執行內，以及下次重新執行時）。
- 伺服器支援 Range 時，大檔用多條連線分段下載；連線數由 `config.toml` 設定，沒設就用預設值。

非目標：多個套件同時下載（方向 D）、HTTP/2、新增依賴、改動 `HttpClient`／token 行為。

## 已確認的限制

- **下載請求絕對不能設 per-request `.timeout()`**：blocking 的 request timeout 會被帶進 async 層的
  `total_timeout`，套用在整個 body（`reqwest-0.12.28/src/async_impl/client.rs:3090`），
  大檔會在 30 秒時被切斷。目前 stall 保護來自 blocking **client 層**的預設 30 秒
  （只作用在拿到 header 前，以及每次 `read()`）。這點維持不變，只把它改成明確設定並寫清楚註解。
- GitHub 資產會 302 轉址到帶簽名、約 33 分鐘後過期的網址 → 每次重試／每個分段都重新請求**原始 URL**。
- reqwest 沒開 `http2` → 每個分段各自是一條 HTTP/1.1 TCP 連線（這正是繞過每連線限速所需要的）。
- 不同套件的同名資產會用到同一個 `downloads_dir/<asset_name>` → 續傳狀態必須比對 URL。

## 第 1 階段：行程內重試＋Range 續傳（commit 1）

`src/downloader/mod.rs`
1. 寫入 `<dest>.part`；完成後檢查長度 == `Content-Length`／`Content-Range` 的總長，再 rename 成 `dest`。
2. 重試迴圈：遇到傳輸錯誤、讀取錯誤、stall timeout、5xx、429 就重試；其他 4xx 直接失敗。
   退避間隔 1s → 2s → 4s；**連續 3 次沒有任何進展**就放棄（只要有進展就重置計數）。
3. 續傳請求：`Range: bytes=<n>-` 加上 `If-Range: <ETag 或 Last-Modified>`。
   - 回 206 且 `Content-Range` 起點 == n → 接續附加。
   - 回 200（伺服器不支援 Range 或檔案已變）→ 截斷成 0，從頭寫。
   - 回 416 → 從頭來。
4. 印出 `Resuming at X MB (attempt k)`；進度條從目前位置繼續。
5. 這一階段放棄時，`.part` 由下載器自己刪除（第 3 階段才改成保留）。

`src/utils/http.rs`：在 `shared_client()` 明確寫 `.timeout(30s)` 並註解 blocking 的語意
（行為不變），修正「no total timeout」的錯誤描述；`docs/plan/BACKLOG.md` SI-12 的描述同步改正。

測試（不新增依賴；在測試裡用 `TcpListener` 寫最小的 HTTP/1.1 伺服器，可設定支援 Range、
ETag、在第 K 個 byte 斷線、忽略 Range、中途換 ETag）：
- 中途斷線 → 下一個請求帶 `Range: bytes=K-`，結果和原檔逐 byte 相同
- 伺服器忽略 Range → 從頭下載，結果正確
- 404 → 不重試、直接報錯
- 一直沒進展 → 3 次後放棄，且 `.part` 已清除
- 長度不符 → 報錯
- 失敗時不會產生 `dest`

## 第 2 階段：切片多連線＋設定（commit 2）

`src/core/preferences.rs`
- 新增 `download_connections: Option<u8>`（不輸出 None）；沒設 = 4。有效範圍 1..=16，1 = 停用切片。
  `validate()` 檢查範圍。預設 `config.toml` 範本加入註解說明。

呼叫端傳入 `DownloadOptions { connections }`：`PackageInstaller`（新增欄位，
`add` 從 `config.preferences()` 帶入）、`install_from_urls`、self-update（`Config::new().ok()`，
失敗就用預設值）。

`src/downloader/`（拆出 `segmented.rs`）
1. 探測：送 `GET Range: bytes=0-`。拿到 206、有總長、總長 ≥ 16 MiB 且 connections > 1
   → 走切片模式；否則沿用第 1 階段的單流。
2. 用 `set_len(total)` 預先配置 `.part`；切成 8 MiB 一塊放進佇列；用 `std::thread::scope`
   開 N 個 worker，以 `AtomicUsize` 取下一塊（與 `update.rs` 相同模式）。
3. 每塊各自請求 `Range: bytes=a-b` + `If-Range`。必須回 206 且起點正確，
   用 `write_all_at`（unix `FileExt`／windows `seek_write` 小 helper）寫到對應位置；
   每塊的重試沿用第 1 階段的規則。
4. 任一塊回 200 或 ETag 改變 → 中止全部分段，改用單流從頭下載。
5. 共用一個進度條（`inc`）。

測試：N=4 加上很小的塊大小，組出的檔案與原檔相同；不支援 Range／小檔 → 單流；
中途換 ETag → 從頭下載且結果正確；設定解析（沒設 = 4、0 和 17 無效、1 = 單流）。

## 第 3 階段：跨次執行續傳（commit 3）

1. 下載失敗時（網路錯誤、Ctrl-C、重試用完）保留 `<dest>.part`，旁邊加
   `<dest>.part.json` = `{url, validator, total, chunk_size, done_chunks}`；
   每完成一塊就用 tmp+rename 原子更新。單流模式直接以 `.part` 的長度作為續傳起點。
2. 開始下載時，如果 sidecar 存在、url 相同、探測拿到的 validator 一致 → 續傳；否則丟棄重下。
3. 對 `.part` 用 `File::try_lock()`（std，Rust ≥ 1.89；行程結束就自動釋放）：
   有其他 wenget 正在下載同一個檔 → 報清楚的錯誤。
4. 每次開始下載時，刪除 `downloads_dir` 裡超過 7 天的 `.part`／`.part.json`。
5. checksum 失敗 → 刪除最終檔（目前 `CleanupGuard` 已這樣做，不變）。
   self-update 的整個暫存目錄仍由 guard 刪除 → 不做跨次續傳（wenget 本身約 2 MB，可接受）。

測試：重試用完 → `.part` 和 sidecar 都留著；第二次呼叫只抓剩下的部分（伺服器看到的 Range
起點正確）；url 或 validator 不符 → 從頭重下；鎖被占用 → 報錯；過期檔被清除。

## 驗證（實際執行檔，sandbox）

`cargo build --release`，然後用 `env WENGET_ROOT=/tmp/wg-dl ...`：
- 安裝一個 >100 MB 的套件，`download_connections = 1` 和 `4` 各計時 ≥3 次比較。
- 下載中途按 Ctrl-C，再重跑同一個指令 → 看到 `Resuming at …`，且 checksum 通過。
- 每個階段都跑：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`。

## 文件（隨各階段的 commit 一起更新）

`CHANGELOG.md`（`## [Unreleased]` → `### 2026-10-01`）、`README.md` 的設定說明、
`docs/reference/glossary.md`（如有新名詞）、`docs/plan/README.md` 的索引、BACKLOG SI-12。

## 驗證結果（2026-10-01，release build，sandbox）

測試檔：hadolint v2.15.1 `hadolint-macos-arm64`（102,616,248 bytes），直接 URL 安裝。

| 連線數 | 完整下載耗時 | 約略速度 |
|---|---|---|
| 4 | 132 s、99 s、74 s、86 s | 0.7–1.3 MiB/s |
| 1 | 310 s、734 s、736 s、718 s | 0.13–0.31 MiB/s |

- 切片模式 Ctrl-C（45 s，完成 4/13 塊）後重跑 → `Resuming previous download at 32.0 MB`，54 s 完成。
- 單流模式 Ctrl-C（60 s，21 MB）後重跑 → `Resuming previous download at 21.0 MB`，271 s 完成。
- 每次結果的 SHA-256 都等於 GitHub 公布的 asset digest；完成後 `cache/downloads/` 沒有殘留檔。
- 備註：非互動 shell 腳本裡的背景程序會忽略 SIGINT，所以第一次批次測試的 `kill -INT` 沒有作用；那幾次就當成完整的 1 連線計時使用。
