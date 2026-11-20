# Danh sách kiểm tra trước khi phát hành

- Trạng thái: bản nháp đầu, ngày 2026-11-21. Chưa có bản phát hành nào; **chưa ai thực hiện các mục thủ công ở phần 2**. Đánh dấu một mục chỉ khi bạn đã tự làm và ghi lại kết quả (ngày, máy, phiên bản, số đo) vào ADR hoặc ghi chú phát hành tương ứng.
- Quy trình phát hành từng bước ở [deployment-guide.md](./deployment-guide.md). Mô hình đe doạ và các mục còn mở ở [threat-model.md](./threat-model.md).
- Nguyên tắc: không phát hành khi còn mục bắt buộc chưa xong, trừ khi ghi rõ trong ghi chú phát hành rằng mục đó chưa được kiểm và vì sao.

## 1. Tự động (phải xanh tại đúng commit sẽ gắn tag)

- [ ] CI xanh trên cả macOS và Windows (job `core`), job `server` và job `licenses`. Nếu job Windows đỏ thì phải tìm ra nguyên nhân, không bỏ qua.
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- [ ] Trong `apps/desktop`: `pnpm typecheck`, `pnpm test`, `pnpm build`.
- [ ] Trong `server`: `cargo fmt --check`, `cargo clippy -- -D warnings`.
- [ ] `cargo deny check` sạch; mọi `ignore` trong `deny.toml` vẫn còn đúng lý do hoặc đã bị xoá.
- [ ] Bộ kiểm thử đồng bộ chạy với server cục bộ: `NE_SYNC_TEST_URL=http://127.0.0.1:8787 cargo test -p ne-sync` (cách chạy ở [SETUP.md](./SETUP.md)).
- [ ] Version khớp ở `tauri.conf.json`, `[workspace.package]` của `Cargo.toml`, `apps/desktop/package.json` và tag; `CHANGELOG.md` đã chuyển mục Unreleased sang version mới.
- [ ] README, SECURITY.md, các tài liệu trong `docs/` khớp với mã tại commit này (đặc biệt phần "works today" và "in progress" của README).
- [ ] Biến `MACOS_FEATURES` và `WINDOWS_FEATURES` ở đầu `release.yml` chỉ chứa feature có thật trong `apps/desktop/src-tauri/Cargo.toml`.

## 2. Thủ công (chưa ai làm)

### 2.1 Chạy thử giao diện trên máy thật

Chưa có lần nào ứng dụng được mở và dùng trong cửa sổ thật; mọi kiểm thử giao diện hiện chạy không có cửa sổ (jsdom).

- [ ] **Máy Mac thật** (ghi rõ Apple Silicon hay Intel, phiên bản macOS), dùng bản cài từ dry run. Đi hết luồng: tạo workspace, tạo trang lồng nhau, gõ nội dung, menu `/`, đủ 15 kiểu block, kéo-thả, đổi kiểu block, tìm `ghi chu` ra `ghi chú`, yêu thích, thùng rác và khôi phục, liên kết trang và backlink, đính kèm ảnh, nhập thư mục Markdown và xuất ra, đóng app mở lại thấy dữ liệu còn nguyên.
- [ ] **Máy Windows thật** (Windows 10 và Windows 11 nếu có), cùng luồng trên với bộ cài NSIS. Kiểm WebView2 có sẵn, và gõ tiếng Việt bằng bộ gõ thật (Telex và VNI) trong editor, vì chưa đo IME.
- [ ] Hai máy đồng bộ với nhau qua server thật: sửa offline trên cả hai rồi nối lại, hội tụ đúng; chỉ báo trạng thái đồng bộ đúng các trạng thái đã chờ, offline, lỗi.
- [ ] Vault: tạo, khoá, mở, tự khoá khi idle và khi máy ngủ, xem và sao chép secret cần mật khẩu cấp 2, clipboard được dọn, nhập CSV từ trình quản lý khác bằng dữ liệu giả.
- [ ] Keychain macOS và Credential Manager Windows thật sự lưu và đọc lại token và khoá gốc; xoá chúng thì app đòi đăng nhập lại.
- [ ] CSP trong webview thật: `loro-crdt` nạp được (WebAssembly), không có lỗi CSP trong console.
- [ ] Độ trễ gõ trong WKWebView và WebView2 thật, p95 dưới 16 ms trên trang 5.000 block, và trên một máy Windows yếu (điều kiện của [ADR 0006](./decisions/0006-page-document-structure.md)).

### 2.2 Cài mới rồi cập nhật

- [ ] Cài sạch trên máy chưa từng có app, mở lần đầu đúng như hướng dẫn trong README (macOS: Open Anyway và cách `xattr`; Windows: SmartScreen More info, Run anyway). Nếu hướng dẫn lệch với màn hình thật của phiên bản hệ điều hành hiện tại, sửa README.
- [ ] Cài bản trước (hoặc một bản thử), tạo dữ liệu, cài bản mới đè lên: dữ liệu, đăng nhập và cài đặt còn nguyên.
- [ ] Gỡ cài đặt: ghi lại cái gì còn lại trên máy (thư mục dữ liệu, mục keychain) và nói rõ trong tài liệu.
- [ ] Nếu đã tích hợp updater: cập nhật từ bản cũ lên bản mới qua `latest.json`, và thử một chữ ký sai bị từ chối. Nếu chưa, ghi rõ trong ghi chú phát hành rằng cập nhật là thủ công.

### 2.3 Server trên Cloudflare thật

Làm theo đúng [server/README.md](../server/README.md) từ tài khoản trống, không dùng thông tin ngoài tài liệu.

- [ ] Triển khai từ tài khoản trống theo từng bước; ghi lại chỗ tài liệu thiếu hoặc sai và sửa.
- [ ] Đo **CPU mỗi request** trên gói miễn phí: đẩy 100 update một lần và kéo một trang 200 dòng bằng `wrangler tail`, tìm lỗi "exceeded resource limits". Đây là rủi ro lớn nhất của ADR 0004.
- [ ] Xác nhận Durable Object **thật sự ngủ**: thời lượng trong dashboard không tăng khi ba thiết bị kết nối mà không sửa gì.
- [ ] R2 có đòi thẻ thanh toán hay không; hành vi chính xác khi chạm hạn mức ngày.
- [ ] Ghi các số đo thật vào ADR 0004 và README server; đổi các mục "chưa kiểm chứng" thành đã kiểm chứng hoặc đã bác bỏ.
- [ ] Thử lệnh sao lưu một đối tượng R2 và quy trình xoá (đúng môi trường) trên tài khoản thử, không phải tài khoản có dữ liệu thật.

### 2.4 Nhận dạng giọng nói bằng giọng người thật

Mọi số đo hiện tại dùng giọng tổng hợp (ADR 0007).

- [ ] Thu lại bộ câu trong `spikes/voice/sentences/` bằng giọng người thật qua micro laptop (tiếng Việt, tiếng Anh, trộn), chạy `measure-transcription`. Ngưỡng: WER tiếng Việt dưới 12%, tiếng Anh dưới 8%, 10 giây lời nói xử lý dưới 5 giây không dùng GPU rời.
- [ ] Đo trên **laptop Windows không GPU rời** và một Mac cũ hơn M4 Pro. Nếu vượt ngưỡng thì quay lại ADR 0007 để huỷ hoặc đổi model.
- [ ] Biên dịch feature `transcribe` trên runner Windows (cần cmake, MSVC, LLVM) nếu muốn bật cho bản Windows; nếu không, ghi rõ bản Windows không có giọng nói.
- [ ] macOS: bản đóng gói có `NSMicrophoneUsageDescription`, hộp thoại quyền micro hiện đúng, từ chối quyền thì app xử lý nhẹ nhàng.
- [ ] Tải model: tiếp tục tải dở, kiểm SHA-256, rồi chạy được khi tắt mạng hoàn toàn.

### 2.5 Bảo mật

- [ ] **Rà soát bảo mật độc lập** cho `ne-crypto` và `ne-vault`, và cho giao thức đồng bộ. Chưa làm. Nếu phát hành khi chưa có, ghi rõ trong ghi chú phát hành, README và SECURITY.md rằng mã mật mã chưa được audit.
- [ ] Rà soát toàn bộ lệnh IPC của shell desktop; mỗi lệnh có test từ chối tham số xấu (threat-model mục 3.1).
- [ ] Kiểm cách giao diện hiển thị nội dung không tin cậy: link với scheme lạ, tiêu đề và `alt` từ thiết bị khác (threat-model mục 3.1).
- [ ] **Quét lịch sử git tìm secret** trước khi phát hành lần đầu, trên mọi nhánh và mọi tag, bằng một công cụ như `gitleaks` hoặc `trufflehog`; quét cả cây làm việc hiện tại. Gặp gì thì thu hồi secret đó trước, rồi mới tính chuyện viết lại lịch sử.
- [ ] Rà soát tài liệu và mã tìm đường dẫn tuyệt đối cục bộ, tên cá nhân ngoài phần giấy phép, và thông tin nội bộ.
- [ ] Đo Argon2id trên Windows và máy yếu, chốt tham số (ADR 0003); nếu đổi, cập nhật `docs/security-model.md`.
- [ ] Xác thực hai yếu tố cho tài khoản GitHub của maintainer, bảo vệ tag `v*` và nhánh `main`.
- [ ] Hai secret `TAURI_SIGNING_PRIVATE_KEY` và `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` đã được đặt và khoá riêng đã sao lưu tách biệt, hoặc chủ động quyết định phát hành mà không có artifact updater.

## 3. Bản build

- [ ] Chạy workflow Release bằng dispatch (dry run). Đây là lần đầu workflow được chạy, nên đọc kỹ log từng job và sửa workflow nếu cần trước khi gắn tag.
- [ ] Job summary ghi đúng tham số build, đúng feature, và trạng thái updater (có hoặc bỏ qua).
- [ ] Artifact có đủ: `.dmg` (macOS), `-setup.exe` (Windows), `SHA256SUMS.txt`; có `.sig` và `.app.tar.gz` khi có khoá.
- [ ] `SHA256SUMS.txt` khớp khi tải artifact về và kiểm lại bằng các lệnh trong README (macOS và PowerShell).
- [ ] macOS: file chạy là universal (`lipo -archs` cho thấy cả `arm64` và `x86_64`), chữ ký ad-hoc có mặt (`codesign -dv`), và app mở được trên cả Mac Apple Silicon và Mac Intel nếu có.
- [ ] Bản có feature `transcribe` mở được và không đòi thư viện thiếu trên máy sạch.

## 4. Phát hành

- [ ] Tag `vX.Y.Z` đúng version, đẩy lên; workflow tạo release nháp.
- [ ] Duyệt bản nháp: đủ file, ghi chú đúng, giữ cảnh báo bộ cài không ký và cảnh báo chưa audit.
- [ ] Bấm Publish bằng tay.
- [ ] Sau khi công khai: tải từ trang Releases, kiểm checksum, cài thử một lần nữa.
- [ ] Cập nhật SECURITY.md (phiên bản được hỗ trợ), threat-model (mục đã xong hoặc còn mở) và README (trạng thái, liên kết tải).
- [ ] Sẵn sàng cho rollback: biết cách rút release, phát bản vá, và quy tắc không dùng lại số version (deployment-guide mục 5).
