# Hướng dẫn phát hành và triển khai

- Trạng thái: bản nháp đầu, ngày 2026-11-21. Quy trình dưới đây **chưa từng được chạy thật**: workflow `release.yml` chưa thực thi lần nào (phần tạo artifact updater và `latest.json` mới chỉ được kiểm bằng cách chạy riêng từng đoạn script trên file giả và kiểm cú pháp YAML), chưa có tag nào, chưa có khoá ký updater (đó là bước thủ công của maintainer, mục 2.1). Lần đầu hãy chạy dry run (mục 3) và đối chiếu từng bước với [danh sách kiểm tra trước phát hành](./release-checklist.md).
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
   build       : macOS (universal, ad-hoc sign) và Windows x64 (NSIS);
                 có khoá updater thì thêm gói cập nhật đã ký (*.sig)
   checksums   : latest.json cho updater (nếu có chữ ký) và SHA256SUMS.txt cho mọi artifact
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

Thêm hai secret và một biến (variable) vào repository (Settings → Secrets and variables → Actions, hoặc bằng `gh`):

```sh
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/note-everywhere.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD     # nhập mật khẩu khi được hỏi, không đưa vào lệnh
gh variable set UPDATER_PUBLIC_KEY < ~/.tauri/note-everywhere.key.pub
```

`UPDATER_PUBLIC_KEY` là **nội dung nguyên file `.pub`** (một chuỗi base64), không phải bí mật. Workflow ghép nó vào `plugins.updater.pubkey` của cấu hình Tauri cho riêng lần build đó; `tauri.conf.json` được commit **không** chứa khoá. Nhờ vậy khoá công khai nằm trong file thực thi của app và app dùng nó để kiểm chữ ký mọi bản cập nhật.

Quy tắc:

- Không bao giờ commit khoá riêng hoặc mật khẩu của nó. `.gitignore` đã chặn `*.key` và `*.pem` nhưng đừng dựa vào đó.
- Sao lưu khoá riêng và mật khẩu ở nơi tách biệt với máy phát triển (ví dụ trình quản lý mật khẩu và một bản offline). **Mất khoá riêng nghĩa là các bản đã cài không nhận được cập nhật nữa** và người dùng phải cài lại bằng tay. Hiện không có cơ chế xoay khoá, vì khoá công khai đã nằm trong các bản đã cài.
- Workflow chỉ tạo artifact updater khi **cả** secret `TAURI_SIGNING_PRIVATE_KEY` **và** biến `UPDATER_PUBLIC_KEY` cùng tồn tại. Thiếu một trong hai, workflow vẫn chạy thành công, bỏ qua artifact updater và `latest.json`, và ghi một thông báo rõ ràng trong job summary (nếu có secret mà thiếu biến thì thêm một cảnh báo, vì bản build khi đó không thể kiểm chữ ký). Một `UPDATER_PUBLIC_KEY` không phải nội dung file `.pub` hợp lệ làm job build thất bại ngay. Không có secret nào bị in ra log.
- Bản build thiếu khoá công khai (build trên máy, CI, fork) **không crash**: app báo trạng thái cập nhật là `unavailable` kèm lý do và không kết nối mạng để tìm bản mới.

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
   - có `.dmg`, `-setup.exe` và `SHA256SUMS.txt` (cùng `.app.tar.gz`, các file `.sig` và `latest.json` nếu có khoá updater);
   - nếu có `latest.json`: mở nó, kiểm version đúng, ba mục `darwin-aarch64`, `darwin-x86_64`, `windows-x86_64`, và URL trỏ đúng tên asset trong draft;
   - ghi chú tự sinh hợp lý; sửa tay nếu cần, giữ lại đoạn cảnh báo bộ cài không ký;
   - tải về và kiểm lại checksum bằng đúng hướng dẫn trong README.
8. **Bấm Publish.** Sau đó tải thử từ trang Releases và so checksum một lần nữa.

Nếu workflow lỗi giữa chừng sau khi draft đã được tạo, xoá draft cũ trước khi chạy lại job `release`: lệnh tạo release báo lỗi nếu tag đã có release.

## 4. Người dùng cập nhật như thế nào

**Cập nhật trong app (chỉ khi bản phát hành được build với khoá updater).** App dùng plugin updater chính thức của Tauri với endpoint cố định là asset `latest.json` của bản phát hành **mới nhất đã Publish**: `https://github.com/nhatdang2111/note-everywhere/releases/latest/download/latest.json`. Hành vi:

- App kiểm tra khi khởi động, tối đa một lần mỗi ngày (thời điểm lần kiểm tra hoàn tất gần nhất được lưu trong thư mục dữ liệu), và kiểm tra theo yêu cầu bất cứ lúc nào. Lần kiểm tra thất bại (mất mạng) không tính là đã kiểm tra, nên lần khởi động sau thử lại.
- **Không bao giờ tự cài ngầm.** Kiểm tra chỉ cho biết có bản mới. Tải về là một lệnh riêng (có sự kiện tiến độ) và chỉ xác minh chữ ký. Cài và khởi động lại là lệnh thứ ba, do người dùng bấm.
- Gói tải về bị huỷ nếu chữ ký không khớp khoá công khai trong app, nếu nó được ký bằng khoá khác, hoặc nếu nó được ký cho một version khác với version mà `latest.json` công bố.
- Bản build không có khoá công khai báo `unavailable`; người dùng cập nhật thủ công như dưới đây.

`latest.json` chỉ trỏ tới bản **đã Publish**: bản nháp (draft) và bản pre-release (tag có hậu tố như `-rc.1`) không phải "latest" của GitHub, nên không ai nhận chúng qua updater. Workflow tạo `latest.json` cho `darwin-aarch64` và `darwin-x86_64` (cùng một gói universal) và `windows-x86_64`, với URL tải trỏ vào đúng release có tag `vX.Y.Z`. Sau khi Publish bản đầu tiên có updater, hãy kiểm bằng tay (checklist mục 2.2) rằng URL `latest/download/latest.json` trả về đúng file.

**Thủ công.** Người dùng tải bộ cài mới từ trang Releases, kiểm checksum, rồi chạy bộ cài đè lên bản cũ. Dữ liệu nằm trong thư mục dữ liệu của người dùng (do `ne-platform::dirs` xác định) chứ không nằm trong thư mục cài đặt, nên cài đè không nên xoá nó. Điều này **chưa được kiểm chứng** bằng lần cài thật; nó nằm trong danh sách kiểm tra phát hành. Khuyến nghị người dùng sao lưu thư mục dữ liệu trước khi nâng cấp.

**Hệ quả bảo mật của bộ cài không ký kèm bản cập nhật có ký.** Bộ cài không có chữ ký của hệ điều hành, nên macOS Gatekeeper và Windows SmartScreen không xác thực được tác giả; ở lần cài đầu người dùng chỉ có checksum để đối chiếu với trang Releases. Với bản cập nhật, hệ điều hành cũng không kiểm gì thêm: **chữ ký updater, chứ không phải hệ điều hành, là thứ xác thực bản cập nhật**, và gốc tin cậy là khoá công khai được cài sẵn trong app. Hệ quả là ai nắm được khoá riêng ký updater có thể chạy mã tuỳ ý trên mọi máy đã cài, kể cả khi tài khoản GitHub và file `latest.json` không bị đụng tới, nên khoá riêng phải được bảo vệ như khoá phát hành và không được đưa vào log hay máy dùng chung. Ngược lại, kẻ chỉ kiểm soát được `latest.json` hoặc đường truyền (không có khoá riêng) không đưa được gói giả vào app. Xem [threat-model.md](./threat-model.md).

**Thử updater mà không cần khoá thật.** `cargo test -p note-everywhere-desktop --test updater_plugin` tạo một cặp khoá tạm trong bộ nhớ, ký một gói giả, phục vụ nó từ server cục bộ rồi kiểm tra: tải thành công, và chữ ký sai, khoá lạ, gói bị sửa, version không khớp đều bị từ chối. Không có khoá riêng nào được ghi ra đĩa hay vào repository, và các kiểm thử không cài gì cả. Chúng không thay thế việc cập nhật thật từ bản cũ lên bản mới (mục 2.2 của checklist).

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
- **Dữ liệu và hạ cấp.** Schema SQLite chỉ có migration đi lên (`crates/ne-store/migrations/`: `0001_init.sql`, `0002_databases.sql`, và những bản sau) và không có đường hạ cấp. Nếu bản lỗi đã nâng schema, bản cũ có thể không mở được dữ liệu đó. Vì vậy mọi thay đổi schema phải được ghi rõ trong ghi chú phát hành và người dùng được khuyên sao lưu trước.
- **Khi đã có updater.** Đưa release lỗi về draft hoặc pre-release thì `releases/latest` trỏ lại bản Publish trước đó, nên người chưa cập nhật không còn được đề nghị bản lỗi. Người đã cập nhật lên bản lỗi **không** bị hạ cấp (updater chỉ nhận version mới hơn), nên họ cần một bản vá có version cao hơn: hãy phát hành `v0.1.1` nhanh.
- **Server.** Triển khai lại commit tốt gần nhất: checkout tag cũ trong `server/` rồi chạy `npx wrangler@latest deploy --env production`. Dữ liệu trong Durable Object không bị đụng tới, nhưng nếu bản lỗi đã thay đổi lược đồ lưu trữ thì bản cũ có thể không đọc được. Thay đổi giao thức không tương thích phải tăng số phiên bản giao thức (xem [sync-protocol.md](./sync-protocol.md)).

## 6. Server đồng bộ: triển khai, sao lưu, khôi phục

Mỗi người dùng tự triển khai server vào tài khoản Cloudflare của chính họ. Hướng dẫn đầy đủ, từng lệnh, bảng cấu hình và các lệnh xoá nằm ở [server/README.md](../server/README.md); phần này chỉ tóm tắt và không thay thế nó.

**Triển khai** (chạy trong `server/`, luôn dùng `npx wrangler@latest`, không dùng bản `wrangler` cài sẵn): đăng nhập, tạo bucket R2 `note-everywhere-blobs`, `deploy --env production`, đặt secret `SETUP_CODE` (ít nhất 12 ký tự, đọc từ prompt), kiểm tra `/v1/health` bằng User-Agent kiểu app, rồi tạo tài khoản trong app. Tài khoản đầu tiên đóng đăng ký vĩnh viễn. R2 có thể đòi thẻ thanh toán dù chỉ dùng trong hạn mức miễn phí.

**Sao lưu:**
- Các thiết bị là bản sao lưu chính: mỗi thiết bị giữ bản đầy đủ, server chỉ giữ bản mã.
- File đính kèm nằm trong R2; sao chép từng đối tượng bằng `wrangler r2 object get`, hoặc cả bucket bằng công cụ tương thích S3 với token R2 tạo riêng cho việc đó (token này không được đưa vào repository).
- Cơ sở dữ liệu (tài khoản, phiên, log) **không có lệnh xuất**: dữ liệu SQLite của Durable Object chỉ truy cập được từ Worker.
- Recovery key và mật khẩu phải được lưu tách khỏi server. Mất cả hai là mất dữ liệu vĩnh viễn, không ai khôi phục được.

**Khôi phục:** nếu mất cơ sở dữ liệu server, triển khai server mới rồi từ một thiết bị còn dữ liệu cục bộ: đăng xuất (dữ liệu cục bộ được giữ nguyên), sau đó đăng ký hoặc đăng nhập vào server mới. App xếp lại **toàn bộ** dữ liệu cục bộ vào outbox (cây, từng trang, vault, cùng database/template/nhắc hẹn/block đồng bộ) và tải lên lại từ đầu — xem `queue_full_upload` trong `apps/desktop/src-tauri/src/app/session.rs` và mục 13 của [sync-protocol.md](./sync-protocol.md). Các thiết bị khác, nếu vẫn trỏ vào server cũ hoặc đăng nhập lại vào server mới mà không đăng xuất trước, dừng với lỗi `ServerReset` vì lịch sử của server mới nằm sau cursor cục bộ của chúng (`sync.rs`); phải đăng xuất trên từng thiết bị đó rồi đăng nhập lại vào server mới để dữ liệu cục bộ của nó cũng được tải lại.

**Xoá:** các lệnh xoá Worker và bucket là thao tác phá huỷ và không hoàn tác. Trước khi chạy, nêu rõ môi trường đích (`production` hay `dev`) và bảo đảm có recovery key cùng một bản dữ liệu cục bộ. Quy trình chi tiết ở mục "Wipe" của server/README.md.

**Điều local test không cho biết:** CPU mỗi request trên gói miễn phí (giới hạn 10 ms), việc Durable Object có thật sự ngủ khi rảnh, và R2 có đòi thẻ hay không. Đây là các mục đo trên Cloudflare thật trong [release-checklist.md](./release-checklist.md).

## 7. Chi phí và những thứ cố ý không có

| Thứ | Quyết định |
|---|---|
| Chứng chỉ ký code (Apple, Windows) | Không có; bộ cài không ký, hướng dẫn mở lần đầu ở README |
| Công chứng macOS | Không có; chỉ ký ad-hoc |
| Tên miền riêng | Không có; server dùng `workers.dev` của chính người dùng |
| Telemetry, báo lỗi tự động | Không có |
| Cập nhật tự động | Có, nhưng được xác thực bằng chữ ký updater của chính dự án, không phải chữ ký hệ điều hành (mục 4) |
| Kênh phân phối | GitHub Releases cộng tự biên dịch từ mã nguồn |
