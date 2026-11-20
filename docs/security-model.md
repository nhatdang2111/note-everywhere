# Mô hình bảo mật

- Trạng thái: mô tả những gì crate `ne-crypto` đã cài đặt và các giới hạn của nó, ngày 2026-11-21, kể cả khôi phục mật khẩu bằng recovery key phía server (xem `docs/sync-protocol.md` mục 12). Các mục liên quan tới keychain và giao diện vault ghi rõ là thiết kế khi chưa được kiểm chứng.
- Chưa có rà soát bảo mật độc lập. Không có phần nào dưới đây là lời bảo đảm; đây là những gì mã nhằm đạt được và những gì nó không làm.
- Mật mã nằm duy nhất ở `crates/ne-crypto`, chỉ dùng các crate RustCrypto: XChaCha20-Poly1305, Argon2id, HKDF-SHA256, SHA-256, cùng `subtle` và `zeroize`. Không có primitive tự viết.

## 1. Cái gì được bảo vệ

| Dữ liệu | Bảo vệ bằng | Ghi chú |
|---|---|---|
| Nội dung ghi chú (update CRDT) | Khoá workspace, phong bì XChaCha20-Poly1305 | Server chỉ thấy bản mã |
| File đính kèm | Khoá workspace, mã hoá theo khối 64 KiB | Phát hiện cắt cụt, đảo thứ tự |
| Secret trong vault | Khoá vault, bọc bằng mật khẩu cấp 2 | Không mở được bằng mật khẩu đăng nhập |
| Mật khẩu | Không bao giờ rời máy | Server chỉ nhận khoá xác thực rồi lưu bản băm của nó |
| Recovery key | Không bao giờ rời máy | Server chỉ nhận một khoá xác thực dẫn xuất riêng từ nó rồi lưu bản băm (recovery verifier) |

Mục tiêu chính: người vận hành server (chính chủ sở hữu, hoặc ai lấy được dữ liệu server) không đọc được ghi chú và không sửa được chúng mà không bị phát hiện ở mức từng update.

## 2. Hệ thống khoá

```text
mật khẩu tài khoản
   |  Argon2id (salt riêng, tham số lưu cạnh salt)
   v
 master 32 byte
   |  HKDF-SHA256, hai nhãn khác nhau
   +--> khoá xác thực ---------> gửi lên server; server lưu SHA-256(nhãn || khoá)
   +--> khoá bọc (ở lại máy) --> bọc khoá gốc tài khoản (bản "theo mật khẩu")

recovery key tài khoản (200 bit)
   |  HKDF-SHA256, hai nhãn khác nhau
   +--> khoá bọc (ở lại máy) --> bọc khoá gốc tài khoản (bản "theo recovery")
   +--> khoá xác thực recovery -> gửi lên server; server lưu SHA-256(nhãn || khoá)

khoá gốc tài khoản (32 byte ngẫu nhiên, tạo lúc đăng ký)
   |  bọc
   v
khoá workspace (32 byte ngẫu nhiên)
   +--> phong bì update đồng bộ
   +--> khối file đính kèm (khoá con riêng cho mỗi luồng, dẫn xuất bằng HKDF)

mật khẩu cấp 2
   |  Argon2id (salt riêng, tham số riêng) + HKDF
   +--> khoá bọc ------------> bọc khoá vault (bản "theo mật khẩu cấp 2")

recovery key vault (200 bit)
   |  HKDF-SHA256
   +--> khoá bọc ------------> bọc khoá vault (bản "theo recovery")

khoá vault (32 byte ngẫu nhiên): KHÔNG được bọc bởi khoá gốc tài khoản
```

Mỗi lần bọc dùng XChaCha20-Poly1305 với nhãn ngữ cảnh (`WrapContext`) làm dữ liệu xác thực kèm theo. Vì vậy một khoá vault đã bọc không thể đem dùng như khoá gốc đã bọc, và bản "theo mật khẩu" không thể đem dùng như bản "theo recovery".

Hai recovery key (tài khoản và vault) được dẫn xuất bằng nhãn HKDF khác nhau, nên dùng nhầm key này cho phía kia sẽ thất bại thay vì âm thầm thành công.

Trong cùng một recovery key, **khoá xác thực** (nhãn `note-everywhere/v1/recovery/account-auth`) và **khoá bọc** (nhãn `note-everywhere/v1/recovery/account`) là hai đầu ra HKDF-SHA256 độc lập. Đưa khoá xác thực cho server không tiết lộ gì giúp mở khoá bọc: biết một đầu ra không cho biết đầu ra kia mà không biết 200 bit gốc, và khoá xác thực dùng làm khoá bọc không mở được gì (có kiểm thử). Server chỉ lưu verifier (SHA-256 có nhãn) của nó, y như với khoá xác thực của mật khẩu.

Recovery key gồm 25 byte ngẫu nhiên, hiển thị bằng 44 ký tự Crockford base32 chia nhóm bốn, 4 ký tự cuối là checksum 20 bit lấy từ SHA-256. Gõ sai một ký tự bị báo là sai checksum trước khi thử giải mã (một lỗi ngẫu nhiên lọt qua với xác suất 2^-20). Khi nhập, không phân biệt hoa thường, bỏ dấu gạch và khoảng trắng, `O` đọc là `0`, `I` và `L` đọc là `1`.

### Đổi mật khẩu

Đổi mật khẩu bọc lại khoá gốc bằng khoá mới, đổi salt và verifier; dữ liệu không bị mã hoá lại. Hệ quả cần biết: **khoá gốc và khoá workspace không đổi**. Xem mục 5.

### Khôi phục mật khẩu

Khôi phục bằng recovery key đi cùng đường nhưng không cần mật khẩu cũ: client mở khoá gốc **cục bộ** bằng khoá bọc dẫn xuất từ recovery key, bọc lại nó dưới mật khẩu mới với salt mới, và chứng minh với server rằng nó có recovery key bằng khoá xác thực recovery. Server so với recovery verifier, đặt mật khẩu mới, thu hồi mọi phiên và cấp một phiên mới (`docs/sync-protocol.md` mục 12). Khoá gốc, khoá workspace và chính recovery key đều không đổi.

## 3. Server thấy gì, không thấy gì

Server **không** thấy: mật khẩu, khoá mã hoá, khoá gốc/workspace/vault ở dạng chưa bọc, nội dung update, nội dung file đính kèm, nội dung vault.

Server **vẫn** thấy:
- Id workspace, id tài liệu, id thiết bị của từng update (chúng là dữ liệu xác thực kèm theo, không được giấu).
- Kích thước: bản mã update bằng bản rõ cộng 41 byte; file đính kèm bằng kích thước gốc cộng 33 byte đầu và 16 byte mỗi khối. Kích thước suy ra được độ dài gần đúng của trang hay file.
- Thời điểm, tần suất và mẫu hoạt động của từng thiết bị; địa chỉ IP kết nối; số lượng tài liệu và số lần sửa mỗi tài liệu.
- Salt, tham số Argon2, verifier và các khoá đã bọc (cần để client đăng nhập).
- Có tài khoản vault hay không, và lúc nào nó được thay đổi.

Server **có thể**, mà client không chống được bằng mật mã:
- Giữ lại hoặc xoá update, hoặc trả về log cũ (rollback). Phong bì chỉ xác thực từng update, không xác thực thứ tự hay tính đầy đủ của log.
- Phát lại một update đã có vào đúng tài liệu và thiết bị mà nó thuộc về. Update CRDT áp dụng lặp là vô hại về nội dung, nhưng đây là hành vi của lớp trên, không phải của `ne-crypto`.

Server **không thể**, nếu mật mã đúng như thiết kế: chuyển một update sang tài liệu, workspace hoặc thiết bị khác mà vẫn mở được (các id nằm trong dữ liệu xác thực kèm theo, có độ dài đứng trước để không dịch ranh giới được); đảo thứ tự hay bỏ khối giữa luồng file; cắt cụt file mà không bị phát hiện.

Lưu ý: id thiết bị chỉ là nhãn được xác thực, không chứng minh tác giả. Mọi thiết bị đều giữ cùng khoá workspace nên bất kỳ thiết bị nào cũng niêm phong được dưới id thiết bị khác.

## 4. Mô hình đe doạ

### 4.1 Dữ liệu server bị đánh cắp hoặc bị đọc
Kẻ tấn công có salt, tham số, verifier, khoá đã bọc và toàn bộ bản mã.
- Bản mã và khoá đã bọc không mở được nếu không có mật khẩu hoặc recovery key.
- Verifier là SHA-256 có nhãn của khoá xác thực; khoá xác thực và khoá bọc là hai đầu ra HKDF độc lập nên verifier không giúp đoán nhanh hơn khoá bọc.
- **Đoán mật khẩu offline là khả thi**: kẻ tấn công thử từng mật khẩu qua Argon2id rồi kiểm tra bằng khoá gốc đã bọc. Độ an toàn phụ thuộc vào độ dài mật khẩu và chi phí Argon2id (mặc định tạm 256 MiB, 3 vòng; xem `docs/decisions/0003-password-key-derivation.md`, chưa đo trên Windows). Mật khẩu yếu thì Argon2id không cứu được.
- Mật khẩu cấp 2 chịu cùng loại tấn công trên khoá vault đã bọc. Tách biệt hai mật khẩu chỉ có ích nếu chúng thực sự khác nhau; crate không kiểm tra điều này, giao diện phải cảnh báo.
- Recovery key 200 bit không thể dò bằng vét cạn, kể cả khi có recovery verifier: verifier là SHA-256 của một đầu ra HKDF 256 bit của khoá 200 bit nên không có gì để thử nhanh hơn vét cạn 2^200, và nó độc lập với khoá bọc nên không giúp mở các khoá đã bọc.
- Kẻ tấn công chỉ có dữ liệu server không thể đặt lại mật khẩu: việc đó cần khoá xác thực recovery, mà server chỉ giữ verifier của nó. Kẻ chỉ có **kết nối tới server** có thể thử đoán khoá xác thực qua `POST /v1/account/recover`, bị chặn bằng khoá tạm tăng dần theo tên và IP (bộ đếm riêng với đăng nhập), và với 200 bit thì số lần thử không có ý nghĩa.
- Server giả mạo có thể trả tham số Argon2 yếu cho tài khoản đã có để lừa client tạo khoá xác thực dễ dò. `KdfPolicy` chặn việc này bằng sàn tối thiểu (19 MiB, 2 vòng) và trần (1 GiB, 16 vòng, 16 làn) cho mọi tham số đọc từ bản ghi.

### 4.2 Laptop bị đánh cắp khi đang mở khoá
Đây là phần dựa vào thiết kế ở phía app, chưa cài đặt.
- Khoá gốc dự kiến nằm trong keychain macOS / Credential Manager Windows để khỏi gõ mật khẩu mỗi lần; người có phiên đăng nhập OS đang mở sẽ đọc được ghi chú.
- Khoá vault không bao giờ được lưu; vault chỉ đọc được khi nó đang mở trong bộ nhớ. Ngoài thời gian đó, người cầm máy không có mật khẩu cấp 2 thì không mở được.
- Khi vault đang mở, khoá nằm trong RAM. Tự khoá khi idle/sleep giúp rút ngắn khoảng này nhưng không xoá được dấu vết đã ra swap hoặc file hibernate.
- Mã hoá toàn đĩa của hệ điều hành là lớp bảo vệ cho dữ liệu cục bộ khi máy tắt; app không thay thế nó.

### 4.3 Malware chạy cùng người dùng
Không được bảo vệ. Mã độc cùng quyền có thể đọc keychain của người dùng, đọc bộ nhớ tiến trình, ghi phím gõ (nên lấy được cả hai mật khẩu), đọc clipboard. Việc zeroize chỉ giảm thời gian khoá nằm trong bộ nhớ, và là nỗ lực tốt nhất: bản sao do di chuyển giá trị, swap hay allocator có thể còn sót, và bản rõ của update, file chưa được xoá.

### 4.4 Quên mật khẩu
Server không đặt lại được mật khẩu vì không có khoá nào. Cách khôi phục duy nhất là recovery key: một cho tài khoản, một cho vault. **Mất cả mật khẩu và recovery key tương ứng là mất dữ liệu vĩnh viễn**, không ai khôi phục được, kể cả chủ dự án. Với tài khoản, khôi phục làm việc trọn vẹn qua server: nhập tên, recovery key và mật khẩu mới, thiết bị đó đăng nhập, mọi thiết bị khác bị đăng xuất, dữ liệu nguyên vẹn. Khôi phục vault vẫn là thao tác cục bộ bằng recovery key của vault.

Hệ quả bảo mật: **ai có recovery key tài khoản đều chiếm được tài khoản** (đặt mật khẩu mới và đá mọi thiết bị khác ra), và chủ tài khoản không có cách nào vô hiệu hoá một recovery key bị lộ vì recovery key không được xoay (mục 5). Hãy giữ nó như giữ mật khẩu.

## 5. Rà soát mã nội bộ (2026-11-21)

Ngày 2026-11-21, `ne-crypto`, `ne-vault` và các điểm liên quan được đọc lại bằng AI (đọc mã có trợ giúp AI) — **không phải một đợt audit độc lập của con người**. Kết quả: không tìm thấy lỗ hổng khai thác được. Sau đó hai điểm được gia cố:

- So sánh checksum của recovery key (`crates/ne-crypto/src/recovery.rs`, hàm `RecoveryKey::parse`) nay dùng `subtle::ConstantTimeEq` thay vì so sánh thường, nên thời gian so sánh không lộ byte sai đầu tiên.
- File đính kèm SVG không còn được phục vụ như ảnh qua scheme `ne-attachment:` (`img-src`/`media-src` ở mục 3.1 của threat-model.md); SVG không nằm trong danh sách kiểu an toàn của `apps/desktop/src-tauri/src/app/protocol.rs` nên luôn tải xuống dưới dạng file (`Content-Disposition: attachment`, `application/octet-stream`) thay vì được webview dựng như ảnh.

Ghi chú đã chấp nhận, chưa sửa:
- **Bộ truy cập "toàn bộ mục để gộp" của vault** (`Vault::draft_of` trong `crates/ne-vault/src/vault_items.rs`) không đòi xác thực lại gần đây như `reveal`/`copy`/`resolve`/`export_csv`; an toàn của nó dựa vào việc không bao giờ bị lộ ra ngoài crate ứng dụng (dùng để gộp một bản sửa vào mục trước khi niêm phong lại). Giảm nhẹ: kiểu nó trả về (`ItemDraft`) không implement `Serialize` nên không tự nhiên rời khỏi tiến trình, và bị xoá khỏi bộ nhớ khi drop (`Zeroize`, `ZeroizeOnDrop`).
- **Hằng số chính sách KDF nhẹ** (`KdfPolicy::LENIENT` trong `crates/ne-crypto/src/password.rs`) là `pub`, chỉ nới sàn tối thiểu để test và công cụ dùng tham số nhỏ cho nhanh. Doc comment ghi rõ "không bao giờ dùng cho tài khoản thật", nhưng không có gì ở runtime ngăn việc lỡ dùng nó ngoài kỷ luật của người gọi.
- **Đăng ký một lần** (`server/src/account.rs`, hàm `register`) kiểm tra chưa có tài khoản rồi mới ghi tài khoản mới, không có khoá tường minh giữa hai bước; tính đúng đắn dựa vào việc một Durable Object xử lý từng request tuần tự (không hai request nào chạy đồng thời trên cùng một đối tượng), chứ không phải một ràng buộc ở tầng lưu trữ.

## 6. Giới hạn đã biết của phần cài đặt

- **Không xoay khoá.** Đổi mật khẩu không đổi khoá gốc. Nếu kẻ tấn công từng có bản khoá gốc đã bọc và biết mật khẩu cũ (hoặc recovery key), việc đổi mật khẩu sau đó không thu hồi được quyền đó. Recovery key cũng được giữ nguyên khi đổi mật khẩu. Xoay khoá gốc và khoá workspace đòi hỏi mã hoá lại dữ liệu và chưa có.
- **Recovery key không được xoay.** Khôi phục thành công không sinh recovery key mới: cùng khoá vẫn dùng được và vẫn đặt lại được mật khẩu. Một recovery key đã lộ cho tới lần đổi tài khoản; xoay nó (sinh khoá mới, bọc lại khoá gốc, thay verifier) chưa cài đặt.
- **Tài khoản tạo trước khi có recovery verifier** không khôi phục qua server được (`recovery_unavailable`): bản ghi của chúng thiếu verifier. Dự án chưa phát hành nên cách xử lý là tạo tài khoản mới; mọi chức năng khác của tài khoản cũ không đổi.
- **Không thu hồi từng thiết bị.** Mọi thiết bị đã đăng nhập đều có khoá workspace. Thu hồi phiên chỉ chặn truy cập server, không làm thiết bị đó quên khoá.
- **Mật khẩu chỉ bị chặn khi rỗng** trong crate. Độ dài tối thiểu và cảnh báo trùng mật khẩu là việc của giao diện.
- **Giới hạn số lần đăng nhập sai và quản lý phiên** do server đảm nhận: khoá tạm thời tăng dần theo tài khoản và theo địa chỉ IP, token phiên chỉ lưu dạng băm, có hết hạn và thu hồi. Khôi phục bằng recovery key có bộ đếm riêng cùng chính sách, để đăng nhập sai không chặn khôi phục và ngược lại. Chi tiết ở `docs/sync-protocol.md`.
- **Đối tượng lớn** (update và snapshot vượt một dòng) được niêm phong thành các phần độc lập có header (nhóm, chỉ số, tổng số) nằm trong phần được xác thực; server không đảo, bỏ, thay hay trộn phần mà không bị phát hiện (client dừng với `CorruptData`). Server vẫn thấy số phần và kích thước từng phần, tức kích thước gần đúng của đối tượng, cùng loại siêu dữ liệu như với các dòng thường.
- **`last_seen_at` của phiên** là siêu dữ liệu server ghi để hiển thị thiết bị; nó làm lộ cho người vận hành server thời điểm hoạt động gần đúng (làm tròn tới 10 phút), cùng loại thông tin server vốn đã thấy qua dòng log.
- **Luồng file**: giải mã ghi từng khối ra sau khi khối đó xác thực, nhưng một luồng bị cắt hoặc sửa chỉ bị phát hiện khi chạm khối hỏng. Người gọi phải bỏ đầu ra (ví dụ file tạm) nếu có lỗi.
- **Số khối tối đa** theo chỉ số u64, không thực tế để chạm tới.
- **Lỗi bộ sinh số ngẫu nhiên của hệ điều hành làm tiến trình panic** thay vì tiếp tục với khoá hay nonce đoán được được.
- **Nonce ngẫu nhiên 192 bit** cho update và khoá bọc; không có bộ đếm. Xác suất trùng với một khoá có thể bỏ qua ở quy mô một người dùng.
- Module `auth` (dùng trong Worker) biên dịch được cho `wasm32-unknown-unknown`: server phụ thuộc vào `ne-crypto` không kèm tính năng mặc định và CI kiểm tra việc này ở mỗi commit.
- **Vault**: mỗi mục là một bản ghi được niêm phong toàn bộ, kể cả tiêu đề và thẻ; khi hai máy sửa cùng một mục thì một phiên bản trọn vẹn thắng, và xoá luôn thắng sửa. Xem, sao chép và xuất secret đòi nhập lại mật khẩu cấp 2 sau một khoảng thời gian. Kích thước bản ghi đã niêm phong vẫn để lộ độ dài xấp xỉ của mục.
- **Tự khoá vault**: dựa trên thời gian idle và phát hiện máy ngủ bằng cách so đồng hồ; chưa phát hiện được sự kiện khoá màn hình. Phát hiện máy ngủ có thể báo nhầm (app bị treo, đồng hồ nhảy tới) và bỏ sót lần ngủ ngắn hơn 15 giây.
- **Kho bảo mật của hệ điều hành** (login keychain trên macOS, Credential Manager trên Windows) chỉ giữ token phiên và khoá gốc tài khoản để không phải nhập mật khẩu mỗi lần mở app. Khoá vault không bao giờ được lưu ở đó. Phần mềm độc hại chạy cùng tài khoản người dùng có thể đọc được hai giá trị này.

## 7. Ngoài phạm vi

- Nhiều người dùng, chia sẻ, phân quyền hay thu hồi quyền của người khác.
- Che giấu siêu dữ liệu (id, kích thước, thời điểm, IP) khỏi server.
- Bí mật về sau (forward secrecy) hoặc tự phục hồi sau khi lộ khoá: một khoá workspace bị lộ mở được mọi update trong quá khứ và tương lai.
- Chống mã độc cùng quyền người dùng, hệ điều hành bị chiếm, hoặc tấn công kênh phụ trên phần cứng chung.
- Chống lại server cố ý giữ, xoá hoặc trả lại log cũ (mật mã ở đây chỉ phát hiện sửa nội dung từng update).
- Phủ nhận hợp lý (deniability), khoá phần cứng, passkey, sinh trắc học, TOTP.
- Ẩn danh mạng.
