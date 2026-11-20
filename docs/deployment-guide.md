# Hướng dẫn phát hành và triển khai

- Trạng thái: bản nháp đầu, ngày 2026-11-21. Quy trình dưới đây **chưa từng được chạy thật**: workflow `release.yml` chưa thực thi lần nào, chưa có tag nào, chưa có khoá ký updater. Lần đầu hãy chạy dry run (mục 3) và đối chiếu từng bước với [danh sách kiểm tra trước phát hành](./release-checklist.md).
- Đối tượng đọc: người duy trì dự án (maintainer).
- Ràng buộc của dự án (xem [ADR 0005](./decisions/0005-licence-and-distribution.md)): giấy phép MIT, không có chi phí cố định. Vì vậy không có Apple Developer ID, không công chứng (notarization), không chứng chỉ ký code Windows, không tên miền riêng. Bản phát hành là bộ cài **không ký** trên GitHub Releases kèm checksum, cộng với cách tự biên dịch từ mã nguồn. Mỗi người dùng tự triển khai server đồng bộ trên gói miễn phí của Cloudflare. Không có telemetry.

## 1. Tổng quan quy trình

```text
cập nhật version + CHANGELOG
        |
        v
dry run (Run workflow thủ công)  --> tải artifact, kiểm checksum, cài thử
        |
        v
git tag vX.Y.Z  +  git push origin vX.Y.Z
        |
        v
.github/workflows/release.yml
   preflight   : tag = version trong tauri.conf.json = version của workspace
   build       : macOS (universal, ad-hoc sign) và Windows x64 (NSIS)
   checksums   : SHA256SUMS.txt cho mọi artifact
   release     : tạo release NHÁP (draft) với ghi chú tự sinh
        |
        v
maintainer duyệt bản nháp, cài thử, rồi bấm Publish
```

Workflow không tự công khai bản phát hành. Việc bấm Publish luôn là thao tác của người.

Hai điểm cần nhớ về workflow:

- Danh sách cargo feature cho từng nền tảng nằm ở hai biến `MACOS_FEATURES` và `WINDOWS_FEATURES` ở đầu `release.yml`. Tên feature phải tồn tại trong `apps/desktop/src-tauri/Cargo.toml`, nếu không bản build lỗi. Mặc định macOS bật `transcribe,metal`, Windows chỉ dùng feature mặc định (whisper chưa được kiểm biên dịch trên Windows, xem [ADR 0007](./decisions/0007-offline-transcription.md)).
- Bản build macOS là universal binary (Apple Silicon và Intel), chỉ ký ad-hoc. Bản Windows là bộ cài NSIS, không ký.

## 2. Thiết lập một lần

### 2.1 Khoá ký updater của Tauri

Khoá này ký các file cập nhật (`*.sig`) để app đã cài có thể kiểm tra cập nhật đến từ chủ khoá. Nó **không liên quan** tới ký code của macOS hay Windows và không làm cảnh báo Gatekeeper hoặc SmartScreen biến mất.

Tạo cặp khoá, đặt mật khẩu cho khoá riêng khi được hỏi, và lưu ngoài thư mục repository:

```sh
cd apps/desktop
pnpm tauri signer generate -w ~/.tauri/note-everywhere.key
```

Lệnh tạo hai file: khoá riêng (`note-everywhere.key`) và khoá công khai (`note-everywhere.key.pub`).

Thêm hai secret vào repository (Settings → Secrets and variables → Actions, hoặc bằng `gh`):

```sh
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/note-everywhere.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD     # nhập mật khẩu khi được hỏi, không đưa vào lệnh
```

Quy tắc:

- Không bao giờ commit khoá riêng hoặc mật khẩu của nó. `.gitignore` đã chặn `*.key` và `*.pem` nhưng đừng dựa vào đó.
- Sao lưu khoá riêng và mật khẩu ở nơi tách biệt với máy phát triển (ví dụ trình quản lý mật khẩu và một bản offline). **Mất khoá riêng nghĩa là các bản đã cài không nhận được cập nhật nữa** và người dùng phải cài lại bằng tay. Hiện không có cơ chế xoay khoá.
- Nếu hai secret chưa được đặt, workflow vẫn chạy thành công: nó bỏ qua artifact updater và ghi một thông báo rõ ràng trong job summary. Không có secret nào bị in ra log.
- Khoá công khai sẽ phải nằm trong `plugins.updater.pubkey` của `tauri.conf.json` khi app tích hợp updater (xem mục 4). Mục này thuộc phần shell desktop đang được làm, chưa có trong cấu hình hiện tại.

### 2.2 Quyền của GitHub Actions

Workflow dùng quyền tối thiểu: mặc định chỉ `contents: read`, và chỉ job `release` có `contents: write`. Không cần bật thêm gì ở Settings → Actions ngoài việc cho phép Actions chạy. Nếu bạn đã đặt "Workflow permissions" của repository là read-only thì vẫn đủ, vì job `release` tự khai báo quyền ghi của nó.

## 3. Quy trình phát hành từng bước

1. **Hoàn tất danh sách kiểm tra.** Đi hết [release-checklist.md](./release-checklist.md), đặc biệt các mục thủ công chưa ai làm.
2. **Đồng bộ version.** Ba chỗ phải trùng nhau:
   - `version` trong `apps/desktop/src-tauri/tauri.conf.json`,
   - `version` trong `[workspace.package]` của `Cargo.toml` gốc,
   - `version` trong `apps/desktop/package.json` (workflow không kiểm chỗ này, nhưng nên giữ khớp).

   Job `preflight` làm bản build thất bại nếu tag, `tauri.conf.json` và workspace không khớp. Sau khi sửa, chạy `cargo check --workspace` để `Cargo.lock` cập nhật và commit cùng.
3. **Cập nhật `CHANGELOG.md`.** Chuyển nội dung mục Unreleased sang mục của version mới kèm ngày, tạo lại mục Unreleased trống.
4. **Dry run.** Trên GitHub: Actions → Release → Run workflow, chọn nhánh `main`. Chạy thủ công thì workflow build đủ và tải artifact lên, nhưng **không tạo release**. Tải artifact `release-bundle`, kiểm `SHA256SUMS.txt`, cài thử trên máy Mac thật và máy Windows thật (các bước mở lần đầu ở README). Các bản dry run chỉ giữ artifact 14 ngày.
5. **Tạo tag và đẩy lên.** Tag phải là `v` cộng version, ví dụ `v0.1.0`. Tag có hậu tố (ví dụ `v0.1.0-rc.1`) tự được đánh dấu pre-release.

   ```sh
   git tag -a v0.1.0 -m "Note Everywhere 0.1.0"
   git push origin v0.1.0
   ```
6. **Chờ workflow.** Các job `preflight`, `build` (macOS, Windows), `checksums`, `release` chạy theo thứ tự. Kiểm job summary: nó liệt kê tham số build, feature đã bật, và cho biết artifact updater được tạo hay bị bỏ qua.
7. **Duyệt bản nháp.** Vào Releases, mở draft:
   - có `.dmg`, `-setup.exe` và `SHA256SUMS.txt` (cùng `.sig` và `.app.tar.gz` nếu có khoá updater);
   - ghi chú tự sinh hợp lý; sửa tay nếu cần, giữ lại đoạn cảnh báo bộ cài không ký;
   - tải về và kiểm lại checksum bằng đúng hướng dẫn trong README.
8. **Bấm Publish.** Sau đó tải thử từ trang Releases và so checksum một lần nữa.

Nếu workflow lỗi giữa chừng sau khi draft đã được tạo, xoá draft cũ trước khi chạy lại job `release`: lệnh tạo release báo lỗi nếu tag đã có release.

## 4. Người dùng cập nhật như thế nào

**Hiện tại (thủ công).** Người dùng tải bộ cài mới từ trang Releases, kiểm checksum, rồi chạy bộ cài đè lên bản cũ. Dữ liệu nằm trong thư mục dữ liệu của người dùng (do `ne-platform::dirs` xác định) chứ không nằm trong thư mục cài đặt, nên cài đè không nên xoá nó. Điều này **chưa được kiểm chứng** bằng lần cài thật; nó nằm trong danh sách kiểm tra phát hành. Khuyến nghị người dùng sao lưu thư mục dữ liệu trước khi nâng cấp.

**Tự động (chưa có).** Workflow đã sẵn sàng tạo các file cập nhật đã ký (`*.app.tar.gz`, `*.sig`) khi có khoá, nhưng app **chưa tích hợp** `tauri-plugin-updater`, `tauri.conf.json` chưa có `plugins.updater`, và workflow chưa sinh `latest.json`. Muốn bật cập nhật tự động cần làm đủ ba việc: thêm plugin và quyền trong shell desktop, đặt `pubkey` cùng `endpoints` (dự kiến trỏ tới `latest.json` trong GitHub Release), và thêm bước tạo `latest.json` vào workflow. Cho tới khi đó, đừng nói với người dùng rằng app tự cập nhật.

Lưu ý bảo mật: chữ ký updater chỉ chứng minh bản cập nhật do chủ khoá riêng phát hành. Lần cài đầu tiên vẫn là bộ cài không ký, nên gốc tin cậy là khoá công khai được cài sẵn trong app. Xem [threat-model.md](./threat-model.md).

## 5. Rút lại một bản phát hành lỗi

Chọn theo tình trạng:

- **Bản nháp chưa Publish.** Xoá draft trên giao diện Releases, rồi xoá tag nếu muốn dùng lại tên đó (lệnh xoá tag từ xa là thao tác phá huỷ, kiểm tra kỹ tên tag trước khi chạy):

  ```sh
  git push --delete origin v0.1.0
  git tag -d v0.1.0
  ```
- **Đã Publish.** Đừng xoá release hay tag mà người khác đã tải, vì checksum và liên kết sẽ gãy. Làm theo thứ tự:
  1. Chuyển release về draft hoặc đánh dấu pre-release, và thêm ghi chú đầu trang nói rõ phiên bản có lỗi gì.
  2. Phát hành bản vá với version mới (`v0.1.1`) theo quy trình ở mục 3. Không dùng lại số version đã phát hành.
  3. Người đã cài bản lỗi cài bản vá đè lên, hoặc cài lại bản trước đó từ trang Releases.
- **Dữ liệu và hạ cấp.** Schema SQLite chỉ có migration đi lên (hiện mới có `0001_init.sql`) và không có đường hạ cấp. Nếu bản lỗi đã nâng schema, bản cũ có thể không mở được dữ liệu đó. Vì vậy mọi thay đổi schema phải được ghi rõ trong ghi chú phát hành và người dùng được khuyên sao lưu trước.
- **Khi đã có updater.** Phải sửa `latest.json` về bản tốt trước đó, nếu không app vẫn đề nghị bản lỗi. Mục này áp dụng sau khi tích hợp updater (mục 4).
- **Server.** Triển khai lại commit tốt gần nhất: checkout tag cũ trong `server/` rồi chạy `npx wrangler@latest deploy --env production`. Dữ liệu trong Durable Object không bị đụng tới, nhưng nếu bản lỗi đã thay đổi lược đồ lưu trữ thì bản cũ có thể không đọc được. Thay đổi giao thức không tương thích phải tăng số phiên bản giao thức (xem [sync-protocol.md](./sync-protocol.md)).

## 6. Server đồng bộ: triển khai, sao lưu, khôi phục

Mỗi người dùng tự triển khai server vào tài khoản Cloudflare của chính họ. Hướng dẫn đầy đủ, từng lệnh, bảng cấu hình và các lệnh xoá nằm ở [server/README.md](../server/README.md); phần này chỉ tóm tắt và không thay thế nó.

**Triển khai** (chạy trong `server/`, luôn dùng `npx wrangler@latest`, không dùng bản `wrangler` cài sẵn): đăng nhập, tạo bucket R2 `note-everywhere-blobs`, `deploy --env production`, đặt secret `SETUP_CODE` (ít nhất 12 ký tự, đọc từ prompt), kiểm tra `/v1/health` bằng User-Agent kiểu app, rồi tạo tài khoản trong app. Tài khoản đầu tiên đóng đăng ký vĩnh viễn. R2 có thể đòi thẻ thanh toán dù chỉ dùng trong hạn mức miễn phí.

**Sao lưu:**
- Các thiết bị là bản sao lưu chính: mỗi thiết bị giữ bản đầy đủ, server chỉ giữ bản mã.
- File đính kèm nằm trong R2; sao chép từng đối tượng bằng `wrangler r2 object get`, hoặc cả bucket bằng công cụ tương thích S3 với token R2 tạo riêng cho việc đó (token này không được đưa vào repository).
- Cơ sở dữ liệu (tài khoản, phiên, log) **không có lệnh xuất**: dữ liệu SQLite của Durable Object chỉ truy cập được từ Worker.
- Recovery key và mật khẩu phải được lưu tách khỏi server. Mất cả hai là mất dữ liệu vĩnh viễn, không ai khôi phục được.

**Khôi phục:** nếu mất cơ sở dữ liệu server, tạo lại tài khoản từ một thiết bị còn dữ liệu. Việc này cần app hỗ trợ tải lại toàn bộ dữ liệu lên server mới, tính năng đó **không có trong bản đầu**. Các thiết bị từng đăng nhập vào server cũ sẽ dừng với trạng thái "server was reset" vì lịch sử của server nằm sau lịch sử của chúng.

**Xoá:** các lệnh xoá Worker và bucket là thao tác phá huỷ và không hoàn tác. Trước khi chạy, nêu rõ môi trường đích (`production` hay `dev`) và bảo đảm có recovery key cùng một bản dữ liệu cục bộ. Quy trình chi tiết ở mục "Wipe" của server/README.md.

**Điều local test không cho biết:** CPU mỗi request trên gói miễn phí (giới hạn 10 ms), việc Durable Object có thật sự ngủ khi rảnh, và R2 có đòi thẻ hay không. Đây là các mục đo trên Cloudflare thật trong [release-checklist.md](./release-checklist.md).

## 7. Chi phí và những thứ cố ý không có

| Thứ | Quyết định |
|---|---|
| Chứng chỉ ký code (Apple, Windows) | Không có; bộ cài không ký, hướng dẫn mở lần đầu ở README |
| Công chứng macOS | Không có; chỉ ký ad-hoc |
| Tên miền riêng | Không có; server dùng `workers.dev` của chính người dùng |
| Telemetry, báo lỗi tự động | Không có |
| Kênh phân phối | GitHub Releases cộng tự biên dịch từ mã nguồn |
