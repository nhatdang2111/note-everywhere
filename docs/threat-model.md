# Threat model

- Trạng thái: bản nháp đầu, ngày 2026-11-21. Chưa có rà soát bảo mật độc lập; không phần nào dưới đây là lời bảo đảm. Đây là danh sách những gì dự án đã xét, những gì mã nhằm làm, và những gì còn bỏ ngỏ.
- Tài liệu này xây trên [security-model.md](./security-model.md) và **không lặp lại** nó. Phần mật mã (hệ khoá, phong bì, recovery key, server thấy gì) nằm ở đó; ở đây chỉ trỏ tới. Giao thức ở [sync-protocol.md](./sync-protocol.md), quyết định phân phối ở [ADR 0005](./decisions/0005-licence-and-distribution.md).
- Phạm vi: app desktop (macOS, Windows), server đồng bộ do từng người dùng tự triển khai, và đường phân phối bộ cài. Mô hình là **một người, nhiều thiết bị**; không có người dùng thứ hai, chia sẻ hay phân quyền.
- Cách đọc trạng thái: "đã có" nghĩa là mã tồn tại trong repository và có test; nó không có nghĩa là đã được rà soát hay đã chạy trong ứng dụng thật. Shell desktop (`apps/desktop/src-tauri`) và giao diện đang được hoàn thiện, nên mọi thứ liên quan đến IPC dưới đây mô tả thiết kế và cấu hình hiện tại, chưa phải hành vi đã kiểm chứng.

## 1. Tài sản

| Mã | Tài sản | Vì sao quan trọng | Nằm ở đâu |
|---|---|---|---|
| A1 | Nội dung ghi chú | Mục tiêu chính của dự án là giữ nó riêng tư | Bản rõ trong SQLite cục bộ của từng thiết bị; bản mã trên server |
| A2 | File đính kèm | Như A1 | Bản rõ trên thiết bị; bản mã trong R2 |
| A3 | Secret trong vault | Giá trị cao hơn ghi chú thường | Mỗi mục được niêm phong ngay cả khi lưu cục bộ; khoá vault chỉ có trong RAM khi vault mở |
| A4 | Mật khẩu tài khoản, mật khẩu cấp 2, hai recovery key | Mất là mất dữ liệu; lộ là lộ dữ liệu | Trong đầu người dùng và nơi người dùng tự cất; không rời máy |
| A5 | Khoá gốc tài khoản và token phiên | Mở được ghi chú mà không cần gõ mật khẩu | Keychain macOS / Credential Manager Windows |
| A6 | Tính toàn vẹn và sẵn sàng của dữ liệu | Mất hoặc sửa âm thầm là hỏng sản phẩm | Mọi thiết bị cộng server |
| A7 | Kênh phân phối: bộ cài, GitHub repository, workflow release, khoá ký updater | Ai kiểm soát nó chạy được mã trên máy người dùng | GitHub, secret của repository |
| A8 | Tài khoản Cloudflare của người dùng | Chứa server, R2, và có thể bị tính tiền nếu cấu hình sai | Cloudflare |
| A9 | Siêu dữ liệu (id, kích thước, thời điểm, IP) | Lộ mẫu hoạt động dù nội dung được mã hoá | Server, Cloudflare |

## 2. Tác nhân

| Mã | Tác nhân | Năng lực giả định |
|---|---|---|
| T1 | Người đọc dữ liệu server (nhân viên Cloudflare, kẻ chiếm tài khoản Cloudflare, kẻ lấy được bản sao dữ liệu) | Đọc mọi thứ server lưu, giữ lại, xoá, sửa hoặc phát lại bản ghi |
| T2 | Kẻ tấn công mạng | Gọi API công khai của server, gửi lưu lượng giả, nghe lén nếu TLS hỏng |
| T3 | Phần mềm độc hại cùng quyền người dùng | Đọc keychain, bộ nhớ tiến trình, clipboard, ghi phím |
| T4 | Người cầm máy | Có máy tắt (T4a) hoặc đang mở khoá (T4b) |
| T5 | Tác giả nội dung không tin cậy | File Markdown, CSV, ảnh người dùng nhập vào; tiêu đề hay văn bản đến từ thiết bị khác |
| T6 | Kẻ tấn công chuỗi cung ứng | Chiếm một phụ thuộc (cargo, npm, GitHub Action), tài khoản GitHub hoặc một bản sao giả của bộ cài |
| T7 | Chính maintainer | Lỡ tay: đẩy nhầm secret, phát hành bản lỗi |

## 3. Điểm vào, mối đe doạ và biện pháp hiện có

### 3.1 Webview và IPC

Giao diện chạy trong webview của hệ điều hành (WKWebView, WebView2) và gọi lõi Rust qua lệnh Tauri. Nếu mã script độc chạy được trong webview, nó gọi được mọi lệnh IPC có sẵn.

| Mối đe doạ | Biện pháp hiện có | Còn thiếu hoặc không chắc |
|---|---|---|
| Script lạ chạy trong webview (XSS qua nội dung nhập, tiêu đề đến từ thiết bị khác, link) | CSP của cửa sổ chính: `default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src ipc: http://ipc.localhost`. Không tải nội dung từ xa, không `fetch` ra ngoài từ webview. `withGlobalTauri` tắt. Capability chỉ có `core:default` cho cửa sổ `main`. HTML thô khi nhập Markdown bị đổi thành văn bản, không dựng thành HTML | `wasm-unsafe-eval` là bắt buộc cho `loro-crdt` (ADR 0002, 0006) và nới CSP; `style-src 'unsafe-inline'` cũng vậy. Chưa kiểm chứng rằng mọi chỗ hiển thị nội dung (tiêu đề, link, ảnh, callout) đều thoát ký tự đúng, và chưa kiểm hạn chế scheme của link (ví dụ `javascript:`) |
| Lệnh IPC bị lạm dụng (đọc file tuỳ ý, lộ khoá) | Hiện shell mới có lệnh `app_version`; bề mặt IPC thật chưa tồn tại | Khi các lệnh xuất hiện phải rà soát từng lệnh: kiểm tra tham số ở biên, không trả khoá hay bản rõ không cần thiết, không có lệnh đọc/ghi đường dẫn tuỳ ý. Chưa có test bảo mật cho IPC |
| Thư viện webview của hệ điều hành có lỗ hổng | Hệ điều hành tự vá | Ngoài tầm kiểm soát của dự án |

### 3.2 Server đồng bộ

Mô hình: server chỉ thấy bản mã; chi tiết cái gì thấy và không thấy ở security-model mục 3 và 4.1.

| Mối đe doạ | Biện pháp hiện có | Còn thiếu hoặc không chắc |
|---|---|---|
| T1 đọc hoặc sửa nội dung | Mã hoá phong bì XChaCha20-Poly1305; id workspace, tài liệu và thiết bị là AAD nên không chuyển update sang nơi khác được; file đính kèm xác thực từng khối và địa chỉ SHA-256 | Server vẫn giữ, xoá hoặc phát lại log cũ mà client không phát hiện được bằng mật mã (security-model mục 3, 6) |
| Đoán mật khẩu offline từ dữ liệu server bị lấy | Argon2id; `KdfPolicy` chặn tham số yếu do server giả mạo | Mật khẩu yếu vẫn bị dò được. Tham số 256 MiB, 3 vòng là tạm thời, chưa đo trên Windows (ADR 0003) |
| T2 dò mật khẩu qua API | Khoá tạm tăng dần theo tên tài khoản và theo IP; tên không tồn tại có salt giả ổn định; token phiên chỉ lưu dạng băm, có hạn và thu hồi được | Kẻ biết tên tài khoản có thể khoá chủ tài khoản tối đa một giờ (đã chấp nhận ở ADR 0004) |
| T2 tạo tài khoản lạ trên server vừa triển khai | Đăng ký cần `SETUP_CODE` (≥ 12 ký tự), sai mã bị giới hạn như đăng nhập, sau tài khoản đầu tiên đăng ký đóng vĩnh viễn | Khoảng giữa lúc `deploy` và lúc người dùng tạo tài khoản phụ thuộc vào độ mạnh của mã thiết lập; người dùng tự giữ bí mật mã này |
| T2 lạm dụng từ trình duyệt | Từ chối request có `Origin`, không trả header CORS, yêu cầu `User-Agent` bắt đầu bằng `NoteEverywhere/` | `User-Agent` giả được dễ dàng: đây chỉ là bộ lọc lưu lượng trình duyệt, **không phải** ranh giới bảo mật |
| T2 làm cạn hạn mức miễn phí (từ chối dịch vụ) | Giới hạn kích thước body, số kết nối, số phiên, hạn mức file; khi chạm hạn mức server trả "unavailable" và app giữ hàng đợi rồi thử lại | Chưa có biện pháp chống ai đó biết địa chỉ Worker gửi lưu lượng lớn. Hạn mức Cloudflare bị đốt thì đồng bộ ngừng đến 00:00 UTC; dữ liệu cục bộ không mất. Hành vi chính xác khi chạm hạn mức chưa kiểm chứng (ADR 0004) |
| Kết nối tới server bị nghe lén hoặc giả mạo | HTTPS/WSS; client dùng TLS của hệ điều hành (`native-tls`) | Chưa có ghim chứng chỉ; tin vào kho chứng chỉ của hệ điều hành |
| Rò rỉ qua log | README server nêu `wrangler tail` không chứa mật khẩu, token hay nội dung | Chưa có rà soát mã xác nhận độc lập |

### 3.3 Kênh cập nhật

| Mối đe doạ | Biện pháp hiện có | Còn thiếu hoặc không chắc |
|---|---|---|
| Kẻ tấn công thay bản cập nhật | Hiện **không có** cập nhật tự động: người dùng tải bằng tay. Workflow release có thể tạo chữ ký updater (`.sig`) khi có secret | App chưa tích hợp updater, `tauri.conf.json` chưa có `pubkey`, workflow chưa sinh `latest.json` ([deployment-guide.md](./deployment-guide.md) mục 4). Chữ ký updater, khi có, chỉ chứng minh bản cập nhật do chủ khoá riêng tạo |
| Khoá riêng của updater bị lộ hoặc mất | Lưu ở secret của repository; hướng dẫn sao lưu tách biệt | Không có cơ chế xoay khoá. Lộ khoá cho phép phát hành bản cập nhật giả; mất khoá làm các bản đã cài không cập nhật được |
| Tài khoản GitHub hoặc workflow bị chiếm, phát hành bản độc | Release luôn ở trạng thái nháp để người duyệt; quyền ghi chỉ ở job `release` | Checksum nằm cùng trang với bộ cài nên **không chống được** kẻ kiểm soát trang đó. Chưa bật xác thực hai yếu tố hay bảo vệ tag/nhánh được ghi nhận ở đâu trong repository; cần kiểm tra thủ công. Chưa có chứng thực nguồn gốc bản build (provenance attestation) |

### 3.4 Chuỗi cung ứng và bộ cài không ký

Đây là rủi ro cấu trúc của quyết định "không chi phí cố định".

| Mối đe doạ | Biện pháp hiện có | Còn thiếu hoặc không chắc |
|---|---|---|
| Người dùng bị tập thói quen bỏ qua cảnh báo Gatekeeper và SmartScreen | README hướng dẫn đúng cách mở lần đầu và cách kiểm checksum | Đây là hệ quả không tránh được của bộ cài không ký. Bản sao giả của app ở nơi khác trông giống hệt khi mở lần đầu |
| File tải về bị sửa | `SHA256SUMS.txt` kèm theo; có đường tự biên dịch từ mã nguồn | Checksum chỉ phát hiện hỏng hoặc sửa trên đường truyền, không xác thực người phát hành |
| Phụ thuộc cargo hoặc npm bị cài mã độc | `Cargo.lock` và `pnpm-lock.yaml` được commit, CI dùng `--frozen-lockfile`; `cargo deny` kiểm giấy phép và advisory, cấm registry và git lạ | Chưa có `cargo vet` hay rà soát mã phụ thuộc; không có kiểm tra advisory cho npm. `deny.toml` đang bỏ qua hai advisory về `quick-xml` với lý do ghi tại chỗ |
| GitHub Action bị chiếm | Chỉ dùng action của GitHub, dtolnay, Swatinem, pnpm và tauri-apps; `GITHUB_TOKEN` quyền tối thiểu; secret chỉ đưa vào đúng bước build | Action được ghim theo major tag chứ không theo SHA commit; tag có thể bị trượt. Nên ghim SHA |
| Bản build không tái lập được | Không có | Chưa có build tái lập. Người dùng không thể tự so khớp bộ cài với mã nguồn |

### 3.5 Nhập Markdown và CSV

| Mối đe doạ | Biện pháp hiện có | Còn thiếu hoặc không chắc |
|---|---|---|
| Thư mục Markdown độc hại (symlink trỏ ra ngoài, đường dẫn leo thang, thư mục lồng quá sâu, file khổng lồ) | Bỏ qua mục ẩn và symlink; ảnh được `canonicalize` rồi bắt buộc nằm trong thư mục nhập; độ sâu tối đa 64; mỗi file tối đa 64 MiB | Chưa fuzz bộ phân tích. Tổng dung lượng và số file của cả thư mục chưa có giới hạn riêng |
| HTML thô, link nguy hiểm trong Markdown | HTML thô thành văn bản; link ngoài giữ nguyên ở dạng link | Hạn chế scheme của link chưa được xác minh (xem 3.1) |
| Tên file xuất không hợp lệ hoặc tạo đường dẫn lạ | Xuất dùng tên hợp lệ trên Windows | Chưa có test đối kháng về tên ghi đè |
| Nhập CSV mật khẩu từ trình quản lý khác | Đọc trên văn bản mượn, xoá từng bản ghi sau khi dùng, báo cáo chỉ nêu số dòng, không nêu nội dung | File CSV gốc chứa mật khẩu rõ nằm trên đĩa do người dùng giữ; ứng dụng không xoá hộ |
| Xuất CSV vault | Xem, sao chép và xuất secret đòi nhập lại mật khẩu cấp 2 khi đã quá thời gian ân hạn của chính sách khoá (security-model mục 5) | File xuất là **bản rõ** của mọi secret; ô bắt đầu bằng `=`, `+`, `-`, `@` bị bảng tính hiểu là công thức vì export không đổi giá trị |

### 3.6 File đính kèm

| Mối đe doạ | Biện pháp hiện có | Còn thiếu hoặc không chắc |
|---|---|---|
| Server sửa hoặc đổi file | Địa chỉ nội dung SHA-256 của bản mã, xác thực từng khối 64 KiB, AAD gắn với workspace và id đính kèm; tải xuống ghi ra file tạm rồi mới đổi tên khi mọi khối đúng | Chỉ phát hiện khi tải xuống |
| Tải lên quá lớn làm cạn hạn mức | `BLOB_MAX_BYTES` (mặc định 64 MiB, trần 90 MiB), `BLOB_QUOTA_BYTES` (mặc định 8 GiB) | Hạn mức thuộc về server của người dùng nên người dùng tự chỉnh |
| File độc hại được mở bằng ứng dụng khác hoặc ảnh làm lỗi bộ giải mã của webview | Ảnh chỉ hiển thị qua `img-src 'self' data: blob:` | Chưa có quy tắc về việc mở file bằng chương trình mặc định của hệ điều hành; chưa quyết định cảnh báo người dùng. Lỗi bộ giải mã ảnh của hệ điều hành ngoài tầm dự án |
| Kích thước lộ độ dài | Không che | Đã ghi ở security-model mục 3 |

### 3.7 Tải model giọng nói

| Mối đe doạ | Biện pháp hiện có | Còn thiếu hoặc không chắc |
|---|---|---|
| Model bị thay trên đường tải hoặc ở nguồn | Danh mục model ghim URL, kích thước và SHA-256; kiểm SHA-256 trước khi đổi tên file; TLS của hệ điều hành | Tin vào việc SHA-256 trong danh mục là đúng; nguồn là kho công khai trên Hugging Face |
| Rò rỉ âm thanh | Audio chỉ nằm trong bộ nhớ, không ghi đĩa, không gửi đi; mạng chỉ dùng để tải model theo yêu cầu (ADR 0007) | Quyền micro của hệ điều hành và `NSMicrophoneUsageDescription` chưa được kiểm trong bản đóng gói |

### 3.8 Kho khoá của hệ điều hành và vault

Mô tả và giới hạn ở security-model mục 4.2, 4.3 và 5. Tóm tắt phần liên quan tới điểm vào: token phiên và khoá gốc nằm trong keychain nên T3 đọc được chúng; khoá vault không được lưu; vault tự khoá theo idle và sleep, chưa phát hiện được khoá màn hình trên thực tế (`lock_on_screen_lock` có trong chính sách nhưng ne-platform chưa phát hiện sự kiện này); chính sách mặc định đặt thời gian dọn clipboard là 30 giây (việc giao diện thi hành đúng chính sách này chưa được kiểm chứng).

## 4. Rủi ro đã chấp nhận

| Rủi ro | Lý do chấp nhận |
|---|---|
| Bộ cài không ký, không công chứng | Quyết định không có chi phí cố định (ADR 0005). Giảm nhẹ bằng checksum, hướng dẫn mở lần đầu và đường tự biên dịch |
| Không xoay khoá | Đổi mật khẩu bọc lại khoá gốc chứ không đổi nó; xoay khoá đòi mã hoá lại dữ liệu và chưa có (security-model mục 5). Khoá ký updater cũng không xoay được |
| Mã độc cùng quyền người dùng (T3) | Không thể chống ở tầng ứng dụng; xem security-model 4.3 |
| Siêu dữ liệu hiển thị với server | Id, kích thước, thời điểm, IP không được che (security-model mục 6) |
| Server giữ, xoá hoặc trả log cũ | Mật mã ở đây chỉ phát hiện sửa nội dung từng update |
| Không có bí mật về sau, không thu hồi khoá theo thiết bị | Một khoá workspace lộ mở được mọi update; thu hồi phiên chỉ chặn truy cập server |
| Ghi chú và file đính kèm nằm **không mã hoá** trên đĩa của từng thiết bị | SQLite của app là bản dựng thông thường, không phải SQLCipher; bảo vệ khi máy tắt dựa vào mã hoá toàn đĩa của hệ điều hành. Riêng vault niêm phong từng mục kể cả khi lưu cục bộ |
| Khoá tạm đăng nhập có thể bị lợi dụng để khoá chủ tài khoản tới một giờ | ADR 0004 |
| Lộ máy khi đang mở khoá hoặc khi vault đang mở | Khoá nằm trong RAM; tự khoá chỉ rút ngắn khoảng này |

## 5. Mục còn mở

Xếp theo mức độ ảnh hưởng. Mục nào xong thì chuyển vào [release-checklist.md](./release-checklist.md) hoặc xoá khỏi đây, không để tồn đọng.

1. Rà soát bảo mật độc lập cho `ne-crypto` và `ne-vault` (và giao thức đồng bộ). Cho tới khi có, SECURITY.md và README phải nói rõ là chưa audit.
2. Rà soát toàn bộ lệnh IPC khi shell desktop hoàn thành, kèm test từ chối tham số xấu.
3. Xác minh cách giao diện hiển thị nội dung không tin cậy: thoát ký tự, scheme của link, tiêu đề và `alt` của ảnh đến từ thiết bị khác.
4. Đo Argon2id trên Windows và máy yếu, chốt tham số (ADR 0003).
5. Đo CPU mỗi request, hibernation và hành vi chạm hạn mức trên Cloudflare thật (ADR 0004).
6. Ghim các GitHub Action theo SHA commit; cân nhắc chứng thực nguồn gốc bản build và `cargo vet`.
7. Quyết định và cài đặt đường cập nhật tự động nếu muốn có (khoá, `pubkey`, `latest.json`), cùng quy trình khi lộ khoá.
8. Bật xác thực hai yếu tố, bảo vệ tag `v*` và nhánh `main` trên GitHub; ghi lại cách kiểm tra.
9. Phát hiện sự kiện khoá màn hình cho vault (cần mã nền tảng `unsafe` hiện bị cấm trong workspace, nên cần quyết định riêng).
10. Fuzz bộ phân tích Markdown và CSV; thêm giới hạn tổng dung lượng khi nhập thư mục.
11. Cân nhắc cảnh báo người dùng trước khi mở file đính kèm bằng chương trình ngoài.
12. Quét lịch sử git tìm secret trước lần phát hành đầu (có trong release-checklist).
