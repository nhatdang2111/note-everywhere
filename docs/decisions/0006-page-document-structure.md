# 0006 — Cấu trúc tài liệu trang: ProseMirror + Loro qua `loro-prosemirror`

- Trạng thái: đã chốt ngày 2026-11-21: giữ Loro với `loro-prosemirror` 0.4.4, kèm hai điều kiện ở phần Kết luận

## Quyết định
Mỗi trang là một `LoroDoc` riêng. Editor trong webview là ProseMirror gắn với `LoroDoc` bằng binding chính thức `loro-prosemirror`; webview gửi bytes update của Loro cho lõi Rust, lõi Rust nhập bằng crate `loro` native và đọc/ghi cùng một cấu trúc container. Schema editor nằm ở `apps/desktop/src/editor/schema.ts` và là nguồn sự thật cho tên node, tên mark và thuộc tính.

## Kết luận
**Giữ Loro với `loro-prosemirror`.** Cược kiến trúc đứng vững: cùng một tài liệu được dựng bởi editor (WASM), đọc và sửa bằng crate `loro` native, rồi quay lại editor mà không mất gì (xem Bằng chứng). Gõ phím trên trang 5.000 block đạt ngưỡng 16 ms ở p95. Binding không làm được block di chuyển được và không nhớ thuộc tính khi đổi kiểu; hai điểm đó có cách xử lý nhỏ ở phần Giới hạn và không đủ để đổi sang Yrs.

Hai điều kiện đi kèm:
1. **Áp update từ máy khác không đạt ngưỡng trên trang rất dài.** Ở 5.000 block, p95 là 25,7 ms (ngưỡng 16 ms); đạt từ khoảng 3.000 block trở xuống (p95 9,6 ms ở 2.000 block). Chi phí tăng tuyến tính theo số block gốc. Trong mô hình một người dùng, update từ máy khác chỉ đến khi đồng bộ, nên ảnh hưởng là một lần giật ngắn, không phải mỗi phím. Cách xử lý: gom các update đang chờ thành một lần nhập trước khi editor dựng lại (chưa đo hiệu quả), và coi trang trên khoảng 3.000 block gốc là trang quá dài trong v1 (gợi ý chia trang).
2. **Phải đo lại trong webview thật.** Số đo ở đây là Node/V8 + jsdom, không có layout, paint, IPC và không phải JavaScriptCore (WKWebView của macOS). Biên an toàn của gõ phím là 8,6 so với 16 ms trên M4 Pro; phase 4 phải đo lại trên webview của Tauri, và trên một máy Windows yếu, trước khi coi ngưỡng là đạt.

So sánh với Yrs/`y-prosemirror` (mục cuối của spike trong plan) chưa làm: không cần thiết khi Loro đạt ngưỡng, và chưa có số đo nào cho Yrs. Nếu điều kiện 2 thất bại, đó là phương án thay thế để thử đầu tiên.


## Phiên bản đã ghim
| Thành phần | Phiên bản |
|---|---|
| `loro-crdt` (WASM, phía webview) | 1.18.0 |
| `loro-prosemirror` | 0.4.4 |
| `loro` (Rust, phía lõi) | 1.18.0 |
| `prosemirror-model` / `-state` / `-view` / `-transform` | 1.25.13 / 1.4.4 / 1.43.0 / 1.12.7 |
| `vite-plugin-wasm` | 3.6.0 |
| Node dùng để đo | 22.20.0 (jsdom 31.0.1) |

`loro-crdt` và `loro` phải cùng minor 1.18: định dạng snapshot và update đã được kiểm chứng chéo ở đúng cặp này. Nâng một bên thì phải chạy lại `page_doc_fixture` và `roundtrip-check` (xem cuối tài liệu).

## Cấu trúc container

Binding không dùng cây block phẳng và không có id block riêng. Toàn bộ trang là một cây lồng nhau của `LoroMap` (node) và `LoroList` (danh sách con), cộng `LoroText` cho chữ.

**Root.** Một root container tên `doc`, kiểu Map (`cid:root-doc:Map`). Binding không đụng tới root container nào khác trong cùng `LoroDoc`, nên có thể thêm container riêng (ví dụ metadata) bên cạnh mà không va chạm.

**Mỗi node** (kể cả root) là một `LoroMap` có đúng ba key:

| Key | Kiểu | Nội dung |
|---|---|---|
| `nodeName` | giá trị chuỗi | Tên node trong schema ProseMirror: `doc`, `paragraph`, `heading`, `list_item`, ... |
| `attributes` | container `LoroMap` con | Một key cho mỗi thuộc tính khai báo trong schema, **kể cả khi bằng giá trị mặc định** (`checked: false`, `level: 1`). Node không có thuộc tính vẫn có `attributes` rỗng. |
| `children` | container `LoroList` con | Danh sách con theo thứ tự, xem dưới. |

**Phần tử của `children`** là một trong hai loại:
- `LoroMap` — một node con (block, hoặc node inline không phải chữ như `hard_break`).
- `LoroText` — một đoạn chữ liền nhau. Các text node kế tiếp (dù khác mark) gộp vào **một** `LoroText`; hai `LoroText` chỉ nằm cạnh nhau khi bị ngăn bởi node inline khác. Textblock rỗng có `children` rỗng (không có `LoroText` rỗng).

**Mark inline** là thuộc tính rich text của `LoroText`, đọc bằng `to_delta()` (Rust) hoặc `toDelta()` (JS):
- Key = tên mark (`bold`, `italic`, `code`, `strike`, `link`).
- Value = một map chứa thuộc tính của mark: `{}` cho mark không có thuộc tính, `{"href": "..."}` cho `link`. Không phải `true`.
- Nhiều mark chồng nhau là nhiều key trên cùng một đoạn.

**Kiểu giá trị thuộc tính.** Giữ nguyên kiểu JSON của schema: số nguyên là `I64` (`level`), boolean là `Bool` (`checked`, `open`), chuỗi là `String`. Reader Rust phải chấp nhận cả `Double` cho số vì JS gửi số dưới dạng number.

**Ví dụ** (rút gọn, tên node và thuộc tính đúng như fixture):
```json
{ "doc": {
  "nodeName": "doc", "attributes": {},
  "children": [
    { "nodeName": "heading", "attributes": {"level": 1}, "children": ["Tiêu đề trang"] },
    { "nodeName": "paragraph", "attributes": {},
      "children": ["Xin chào, ", {"nodeName": "hard_break", "attributes": {}, "children": []}, "dòng hai"] },
    { "nodeName": "todo_list", "attributes": {}, "children": [
      { "nodeName": "todo_item", "attributes": {"checked": true},
        "children": [ { "nodeName": "paragraph", "attributes": {}, "children": ["Đã xong"] } ] } ] }
  ] } }
```
(Trong `get_deep_value` / `toJSON` một `LoroText` hiện ra là chuỗi thuần; marks chỉ thấy qua delta.)

### Bảng kiểu block

| Block trong spec | `nodeName` | `attributes` | `children` |
|---|---|---|---|
| Đoạn văn | `paragraph` | — | text và node inline |
| Heading 1–3 | `heading` | `level` (1, 2, 3) | text và node inline |
| Danh sách gạch đầu dòng | `bullet_list` | — | `list_item` (≥ 1) |
| Danh sách đánh số | `ordered_list` | — | `list_item` (≥ 1) |
| Mục danh sách | `list_item` | — | `paragraph` rồi block bất kỳ (cấp lồng nằm ở đây) |
| To-do | `todo_list` > `todo_item` | `todo_item.checked` (bool) | như `list_item` |
| Toggle | `toggle` | `open` (bool) | `paragraph` đầu là tiêu đề, phần còn lại là nội dung gập |
| Quote | `quote` | — | block ≥ 1 |
| Callout | `callout` | `icon` (chuỗi, có thể rỗng) | block ≥ 1 |
| Divider | `divider` | — | rỗng |
| Code | `code_block` | `language` (chuỗi) | một `LoroText`, không mark |
| Ảnh | `image` | `src`, `alt` | rỗng |
| Trang con | `child_page` | `pageId`, `title` | rỗng |
| Liên kết tới trang | `page_link` | `pageId`, `title` | rỗng |
| Xuống dòng mềm (Shift+Enter) | `hard_break` (inline) | — | rỗng |

`title` của `child_page` và `page_link` chỉ là bản sao để hiển thị; nguồn sự thật của tiêu đề là node của trang trong cây workspace. Quan hệ cha con của trang sống trong cây trang, không phải ở `child_page` (spec mục 3).

## Hợp đồng cho reader và writer ở Rust

**Đọc.**
1. `doc.get_map("doc")` → đọc `nodeName`, `attributes`, `children` như bảng trên. Thiếu một trong ba key là tài liệu hỏng, trả lỗi chứ không đoán.
2. Với mỗi phần tử `children`: `Container::Map` là node con, `Container::Text` là chữ. Gặp loại khác là lỗi.
3. Plain text cho tìm kiếm: nối các `LoroText` trong textblock, `hard_break` là `\n`, các block con cách nhau bằng `\n`. Phép tính này đã có test (`plain_text` trong `crates/ne-core/tests/page_doc_fixture.rs`) và giữ nguyên dấu tiếng Việt.
4. Danh sách block và liên kết: duyệt cây theo `nodeName`; liên kết trang lấy từ `child_page` / `page_link` (`pageId`) và từ mark `link` có `href` trỏ tới trang.
5. Reader Rust nên bỏ qua node có `nodeName` lạ thay vì lỗi, và không được xoá chúng khi ghi. Editor thì không khoan dung: theo mã nguồn binding, gặp `nodeName` không có trong schema là ném lỗi khi dựng tài liệu (chưa thử). Vì vậy mọi `nodeName` hoặc mark mới phải ra cùng bản editor biết nó, trước khi Rust bắt đầu ghi.

**Ghi.** Mọi node phải đủ ba key. Thiếu `attributes` thì editor sẽ tự tạo khi đọc, nghĩa là sinh thêm thao tác cục bộ ngay lúc nhập update ngoài. Quy tắc:
- Tạo node: `children_list.push_container(LoroMap::new())`, rồi `insert("nodeName", ...)`, `insert_container("attributes", LoroMap::new())` (ghi đủ mọi thuộc tính của schema, kể cả mặc định), `insert_container("children", LoroList::new())`.
- Chữ: `children.push_container(LoroText::new())` rồi `insert`; mark bằng `LoroText::mark(range, "bold", <map rỗng>)`, `link` mang map `{"href": ...}`.
- Gọi `doc.config_text_style` với cùng cấu hình như phía JS trước khi `mark` hoặc sửa chữ có mark: `bold`, `italic`, `code`, `strike` dùng `ExpandType::After`; `link` dùng `ExpandType::None` (vì schema đặt `inclusive: false`). Cấu hình này không nằm trong dữ liệu và không đồng bộ; nó chỉ quyết định chữ gõ sát mép mark có dính mark hay không. `mark` vẫn chạy khi thiếu cấu hình nhưng ranh giới sẽ khác phía JS.
- Textblock rỗng thì không tạo `LoroText`.
- Chỉ số vị trí: `LoroText` ở Rust tính theo ký tự Unicode (`insert`, `mark`, `len_unicode`), còn `loro-crdt` ở JS và ProseMirror tính theo UTF-16. Hai cách chỉ khác nhau với ký tự ngoài BMP (emoji, một số chữ Hán hiếm); đừng truyền chỉ số qua ranh giới Rust/JS. Chỉ truyền bytes update.
- Sau khi ghi gọi `doc.commit()` rồi `export(ExportMode::updates(&version_trước_khi_sửa))`.
- **Tạo trang chỉ một lần.** Hai thiết bị cùng tạo bộ khung root cho cùng một trang thì key `children` bị ghi đè kiểu last-writer-wins và các block của một bên biến mất (test `independently_created_roots_do_not_merge`). Lõi Rust tạo bộ khung (`nodeName`, `attributes`, `children`, thêm một `paragraph` rỗng) ngay khi tạo trang và đồng bộ nó như update đầu tiên; editor chỉ mở tài liệu đã có khung. Không để editor tự khởi tạo trên `LoroDoc` rỗng khi tài liệu có thể đã tồn tại trên máy khác.

## Bằng chứng

| Kiểm tra | Cách chạy | Kết quả |
|---|---|---|
| JS tạo trang có 15 kiểu block, snapshot và update gia tăng, Rust đọc đúng thứ tự block, text có dấu, lồng danh sách, cờ to-do, mark | `node spikes/page-doc-fixture.mjs` rồi `cargo test -p ne-core --test page_doc_fixture` | 9 test pass |
| Rust sửa chữ trong một block, thêm một paragraph có mark đậm, xuất update; editor nhập update và hiển thị đúng, cả khi mở mới lẫn khi nhập trực tiếp lúc editor đang mở; mở editor trên tài liệu do Rust ghi không sinh thêm thao tác | `node spikes/roundtrip-check.mjs` (sau test Rust) | pass |
| Markdown shortcut (`#`, `-`, `1.`, `[]`, `>`, ba backtick, `---`, `**x**`), Enter/Tab/Shift-Tab trong danh sách, Shift+Enter, chèn đủ các kiểu block, undo | `node spikes/editor-behaviour-check.mjs` (module `src/editor` thật trên jsdom) | pass |
| Ba giả định của ADR: node thiếu `attributes` làm editor ghi vào tài liệu; root container khác `doc` không bị đụng; `loro-crdt` JS đếm chỉ số theo UTF-16 | `node spikes/dump-structure.mjs` (cũng in cấu trúc container) | cả ba xác nhận |
| Editor mở được trong Chrome headless qua `pnpm dev` (WASM của `loro-crdt` nạp qua `vite-plugin-wasm`, binding khởi tạo, không lỗi console) | `pnpm dev` + Chrome `--headless --dump-dom` | pass; chưa thử trong cửa sổ Tauri (CSP, xem Giới hạn) |
| Undo/redo của binding chỉ hoàn tác sửa đổi của máy này, không đụng update nhập từ ngoài | cùng `roundtrip-check.mjs` | pass |

## Đo độ trễ

Chạy bằng `node spikes/typing-latency.mjs`. Máy đo: Apple M4 Pro, 12 lõi, 24 GiB RAM, macOS (Darwin 25.5), Node 22.20.0, V8. Trong lúc đo máy còn chạy các tiến trình khác (load average khoảng 4), nên số đo có nhiễu.

**Tài liệu.** 5.000 block gốc (70% đoạn văn, 10% heading, 10% bullet list hai mục, 5% to-do, 5% quote; một phần đoạn có chữ đậm), tổng 15.832 node, snapshot 1.394 KiB. Dựng qua một transaction ProseMirror mất 0,7–1,0 s; mở editor từ snapshot mất 176–237 ms. Sinh bằng PRNG cố định.

**Phương pháp.** 100 mẫu khởi động rồi 1.000 mẫu; mỗi mẫu chèn một ký tự ở vị trí ngẫu nhiên, cách nhau 10 ms (đủ để vòng lặp sự kiện chạy, xem dưới).
- *local*: `state.apply(tr)` qua `LoroSyncPlugin` (transaction → thao tác Loro → commit) cộng `doc.export` update gia tăng. Đây là phần binding thêm vào.
- *dom*: `view.updateState` trên jsdom, tính riêng.
- *remote*: một peer khác sinh sẵn 1.100 update; đo từ `doc.import` tới khi trạng thái editor đổi, và tới khi việc trì hoãn của binding (khôi phục selection bằng `setTimeout(0)`) chạy xong ("idle"). Số "idle" là số so với ngưỡng.

Kết quả (ms, n = 1.000):

| Phép đo | p50 | p95 | p99 | max |
|---|---|---|---|---|
| local: binding + export | 7,26 | **8,65** | 12,85 | 30,7 |
| local: cập nhật DOM jsdom (tham khảo) | 0,50 | 0,67 | 0,74 | 0,97 |
| local: binding + export + jsdom | 7,76 | 9,25 | 13,82 | 31,2 |
| remote: import → trạng thái editor đổi | 8,37 | 12,06 | 15,14 | 423 |
| remote: import → idle | 16,52 | **25,67** | 227,7 | 614 |

Một update gõ một ký tự dài trung bình 94 byte.

**Đạt/không đạt so với ngưỡng 16 ms p95:** gõ phím (local) **đạt**, 8,65 ms (9,25 ms nếu cộng jsdom). Áp update từ máy khác (remote) **không đạt** ở 5.000 block, 25,67 ms; p99 và max bị kéo lên bởi vài lần dừng dài hàng trăm ms (nhiều khả năng là GC, chưa xác minh).

**Theo kích thước trang** (n = 300 mẫu cho 500–2.000 block, n = 1.000 cho 5.000):

| Block gốc | local p50 | local p95 | remote idle p50 | remote idle p95 |
|---|---|---|---|---|
| 500 | 1,53 | 5,12 | 2,86 | 3,19 |
| 1.000 | 2,68 | 6,11 | 4,03 | 5,00 |
| 2.000 | 6,46 | 8,75 | 6,56 | 9,59 |
| 5.000 | 7,26 | 8,65 | 16,52 | 25,67 |

Remote tăng tuyến tính theo số block (khoảng 5 ms mỗi 1.000 block) và vượt 16 ms ở khoảng 3.000 block. Local cũng tăng nhưng chậm hơn và nhiễu nhiều hơn (khoảng cách 10 ms giữa các phím làm CPU hạ xung, nên số nhỏ không giảm tới mức thấp nhất có thể).

**Nguyên nhân chi phí (từ profile, chưa vá binding để xác minh).** Với Loro thuần (không ProseMirror) mỗi lần sửa chỉ tốn 0,03 ms và không đổi theo số sửa (`spikes/diag-loro-only.mjs`). Phần tốn nằm ở phía JS của binding: profile CPU của vòng lặp gõ trên 5.000 block cho thấy bộ gom rác chiếm áp đảo, vì mỗi lần gõ binding duyệt các block gốc và cấp phát một handle WASM cho mỗi block, và các handle này chỉ được giải phóng qua `FinalizationRegistry`.

**Cạm bẫy khi đo.** Nếu vòng lặp gõ chạy đồng bộ, không nhường vòng lặp sự kiện, các callback của `FinalizationRegistry` không chạy và độ trễ tăng đều từ 4,5 ms lên hơn 100 ms sau 1.000 mẫu (p95 186 ms). Khi nhường vòng lặp (kể cả 1 ms) độ trễ phẳng (`spikes/diag-drift.mjs`). Người gõ thật luôn nhường vòng lặp, nên số trên đã bỏ hiện tượng này; nhưng thao tác dán hoặc import hàng nghìn thay đổi liên tiếp trong một tác vụ đồng bộ sẽ gặp lại nó, nên phải chia nhỏ và nhường vòng lặp giữa các lô.

**Giới hạn của phép đo headless.**
- Không có layout và paint của trình duyệt; jsdom chỉ đo việc cập nhật DOM, rẻ hơn nhiều so với DOM thật.
- V8 trong Node, không phải JavaScriptCore của WKWebView (macOS) hay WebView2 (Windows). JIT, GC và WASM khác nhau.
- Không gồm thời gian IPC của Tauri; update 94 byte nhỏ nhưng chưa đo.
- Chỉ một ký tự chèn ở vị trí ngẫu nhiên. Chưa đo IME tiếng Việt (composition), dán nhiều block, kéo-thả, và tài liệu lồng sâu.
- Một máy, máy đang tải, mỗi cấu hình một lần chạy.


## Giới hạn và cách xử lý

1. **Block không có id ổn định và không di chuyển được như một đơn vị.** `children` là `LoroList`, không phải `LoroMovableList`, và `loro-prosemirror` chỉ biết xoá rồi tạo lại. Kéo-thả một block (spec mục 6) thành xoá một `LoroMap` và tạo một `LoroMap` mới có container id mới. Hệ quả: chỉnh sửa đồng thời của máy khác vào block đang được kéo sẽ rơi vào container đã xoá và mất; trong mô hình một người dùng nhiều máy, tình huống này hiếm và có thể chấp nhận. Cách rẻ nhất nếu cần id: thêm thuộc tính `blockId` (UUID sinh khi tạo block) vào schema cho block cần được tham chiếu (backlink tới block); id này sống qua thao tác di chuyển vì nó được sao chép theo node.
2. **Đổi kiểu block làm mất thuộc tính của kiểu cũ.** `setNodeMarkup` của ProseMirror chỉ giữ thuộc tính có trong kiểu mới, nên heading → to-do → heading không nhớ `checked` như CM-04 yêu cầu. Chưa kiểm chứng cách xử lý; hướng nhỏ nhất là thêm một thuộc tính `stash` (chuỗi JSON) cho mọi textblock, ghi vào đó thuộc tính của kiểu cũ khi đổi kiểu và khôi phục khi đổi lại.
3. **Chưa thấy giới hạn về lồng container.** Toggle, quote, callout và danh sách lồng nhau (kể cả `bullet_list` trong `list_item` trong `bullet_list`) đều đã qua vòng JS → Rust → JS.
4. **Hai thiết bị không được cùng khởi tạo một trang** (xem phần Ghi).
5. **CSP của webview.** `apps/desktop/src-tauri/tauri.conf.json` hiện đặt `default-src 'self'`, chặn biên dịch WebAssembly. Phải thêm `script-src 'self' 'wasm-unsafe-eval'` thì `loro-crdt` mới chạy trong Tauri. Việc này thuộc phần `src-tauri`, spike này không sửa.
6. **Bundler.** `loro-crdt` là module WASM ES; Vite cần `vite-plugin-wasm` và `build.target: "esnext"` (top-level await; Safari 15+ / WebView2). Đã cấu hình trong `apps/desktop/vite.config.ts`.
7. **Cursor đồng bộ** (`LoroEphemeralCursorPlugin`) không dùng: spec không có cộng tác cùng lúc.

## Tái lập
Từ `apps/desktop`:
```
pnpm typecheck
node spikes/page-doc-fixture.mjs         # ghi crates/ne-core/tests/fixtures/*.loro
cargo test -p ne-core --test page_doc_fixture   # đọc fixture, ghi update của Rust ra thư mục tạm
node spikes/roundtrip-check.mjs          # nhập update của Rust vào editor
node spikes/typing-latency.mjs           # đo độ trễ (NE_BENCH_BLOCKS, NE_BENCH_SAMPLES, NE_BENCH_PACE_MS)
node spikes/editor-behaviour-check.mjs   # shortcut, keymap, chèn block
node spikes/dump-structure.mjs           # in cấu trúc container và kiểm ba giả định
```
Update của Rust được ghi vào `<tmpdir>/ne-page-doc-rust-update.loro`, đổi bằng biến `NE_PAGE_DOC_RUST_UPDATE` (cả hai bên cùng đọc biến này). Fixture dùng peer id cố định nên byte đầu ra lặp lại được giữa các lần chạy.
