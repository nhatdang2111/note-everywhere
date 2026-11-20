# Mô hình bảo mật

- Trạng thái: mô tả những gì crate `ne-crypto` đã cài đặt và các giới hạn của nó, ngày 2026-11-21. Phần server, keychain và giao diện vault chưa có; các mục liên quan ghi rõ là thiết kế, chưa kiểm chứng.
- Chưa có rà soát bảo mật độc lập. Không có phần nào dưới đây là lời bảo đảm; đây là những gì mã nhằm đạt được và những gì nó không làm.
- Mật mã nằm duy nhất ở `crates/ne-crypto`, chỉ dùng các crate RustCrypto: XChaCha20-Poly1305, Argon2id, HKDF-SHA256, SHA-256, cùng `subtle` và `zeroize`. Không có primitive tự viết.

## 1. Cái gì được bảo vệ

| Dữ liệu | Bảo vệ bằng | Ghi chú |
|---|---|---|
| Nội dung ghi chú (update CRDT) | Khoá workspace, phong bì XChaCha20-Poly1305 | Server chỉ thấy bản mã |
| File đính kèm | Khoá workspace, mã hoá theo khối 64 KiB | Phát hiện cắt cụt, đảo thứ tự |
| Secret trong vault | Khoá vault, bọc bằng mật khẩu cấp 2 | Không mở được bằng mật khẩu đăng nhập |
| Mật khẩu | Không bao giờ rời máy | Server chỉ nhận khoá xác thực rồi lưu bản băm của nó |

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
   |  HKDF-SHA256
   +--> khoá bọc ------------> bọc khoá gốc tài khoản (bản "theo recovery")

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

Recovery key gồm 25 byte ngẫu nhiên, hiển thị bằng 44 ký tự Crockford base32 chia nhóm bốn, 4 ký tự cuối là checksum 20 bit lấy từ SHA-256. Gõ sai một ký tự bị báo là sai checksum trước khi thử giải mã (một lỗi ngẫu nhiên lọt qua với xác suất 2^-20). Khi nhập, không phân biệt hoa thường, bỏ dấu gạch và khoảng trắng, `O` đọc là `0`, `I` và `L` đọc là `1`.

### Đổi mật khẩu

Đổi mật khẩu bọc lại khoá gốc bằng khoá mới, đổi salt và verifier; dữ liệu không bị mã hoá lại. Hệ quả cần biết: **khoá gốc và khoá workspace không đổi**. Xem mục 5.

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
- Recovery key 200 bit không thể dò bằng vét cạn.
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
Server không đặt lại được mật khẩu vì không có khoá nào. Cách khôi phục duy nhất là recovery key: một cho tài khoản, một cho vault. **Mất cả mật khẩu và recovery key tương ứng là mất dữ liệu vĩnh viễn**, không ai khôi phục được, kể cả chủ dự án. Sau khôi phục bằng recovery key, nên đặt mật khẩu mới ngay (`change_password`).

## 5. Giới hạn đã biết của phần cài đặt

- **Không xoay khoá.** Đổi mật khẩu không đổi khoá gốc. Nếu kẻ tấn công từng có bản khoá gốc đã bọc và biết mật khẩu cũ (hoặc recovery key), việc đổi mật khẩu sau đó không thu hồi được quyền đó. Recovery key cũng được giữ nguyên khi đổi mật khẩu. Xoay khoá gốc và khoá workspace đòi hỏi mã hoá lại dữ liệu và chưa có.
- **Không thu hồi từng thiết bị.** Mọi thiết bị đã đăng nhập đều có khoá workspace. Thu hồi phiên chỉ chặn truy cập server, không làm thiết bị đó quên khoá.
- **Mật khẩu chỉ bị chặn khi rỗng** trong crate. Độ dài tối thiểu và cảnh báo trùng mật khẩu là việc của giao diện.
- **Giới hạn số lần đăng nhập sai và quản lý phiên** do server đảm nhận: khoá tạm thời tăng dần theo tài khoản và theo địa chỉ IP, token phiên chỉ lưu dạng băm, có hết hạn và thu hồi. Chi tiết ở `docs/sync-protocol.md`.
- **Luồng file**: giải mã ghi từng khối ra sau khi khối đó xác thực, nhưng một luồng bị cắt hoặc sửa chỉ bị phát hiện khi chạm khối hỏng. Người gọi phải bỏ đầu ra (ví dụ file tạm) nếu có lỗi.
- **Số khối tối đa** theo chỉ số u64, không thực tế để chạm tới.
- **Lỗi bộ sinh số ngẫu nhiên của hệ điều hành làm tiến trình panic** thay vì tiếp tục với khoá hay nonce đoán được được.
- **Nonce ngẫu nhiên 192 bit** cho update và khoá bọc; không có bộ đếm. Xác suất trùng với một khoá có thể bỏ qua ở quy mô một người dùng.
- Module `auth` (dùng trong Worker) biên dịch được cho `wasm32-unknown-unknown`: server phụ thuộc vào `ne-crypto` không kèm tính năng mặc định và CI kiểm tra việc này ở mỗi commit.
- **Vault**: mỗi mục là một bản ghi được niêm phong toàn bộ, kể cả tiêu đề và thẻ; khi hai máy sửa cùng một mục thì một phiên bản trọn vẹn thắng, và xoá luôn thắng sửa. Xem, sao chép và xuất secret đòi nhập lại mật khẩu cấp 2 sau một khoảng thời gian. Kích thước bản ghi đã niêm phong vẫn để lộ độ dài xấp xỉ của mục.
- **Tự khoá vault**: dựa trên thời gian idle và phát hiện máy ngủ bằng cách so đồng hồ; chưa phát hiện được sự kiện khoá màn hình. Phát hiện máy ngủ có thể báo nhầm (app bị treo, đồng hồ nhảy tới) và bỏ sót lần ngủ ngắn hơn 15 giây.
- **Kho bảo mật của hệ điều hành** (login keychain trên macOS, Credential Manager trên Windows) chỉ giữ token phiên và khoá gốc tài khoản để không phải nhập mật khẩu mỗi lần mở app. Khoá vault không bao giờ được lưu ở đó. Phần mềm độc hại chạy cùng tài khoản người dùng có thể đọc được hai giá trị này.

## 6. Ngoài phạm vi

- Nhiều người dùng, chia sẻ, phân quyền hay thu hồi quyền của người khác.
- Che giấu siêu dữ liệu (id, kích thước, thời điểm, IP) khỏi server.
- Bí mật về sau (forward secrecy) hoặc tự phục hồi sau khi lộ khoá: một khoá workspace bị lộ mở được mọi update trong quá khứ và tương lai.
- Chống mã độc cùng quyền người dùng, hệ điều hành bị chiếm, hoặc tấn công kênh phụ trên phần cứng chung.
- Chống lại server cố ý giữ, xoá hoặc trả lại log cũ (mật mã ở đây chỉ phát hiện sửa nội dung từng update).
- Phủ nhận hợp lý (deniability), khoá phần cứng, passkey, sinh trắc học, TOTP.
- Ẩn danh mạng.
