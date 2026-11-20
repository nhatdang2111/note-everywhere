# Note Everywhere — Product spec v1

- Trạng thái: bản nháp đầu, ngày 2026-11-21
- Căn cứ: [kiểm kê tính năng](./research/notion-feature-inventory.md) lập từ tài liệu công khai, và kế hoạch nội bộ của dự án
- Giới hạn: chưa có quan sát bằng tay. Những hành vi đánh dấu "cần kiểm chứng" nằm trong [hands-on checklist](./research/hands-on-checklist.md).

Mã trong ngoặc (ví dụ BLK-06) trỏ về mục tương ứng trong bản kiểm kê.

## 1. Sản phẩm là gì

Note Everywhere là app ghi chú local-first cho macOS và Windows, dành cho một người dùng trên nhiều máy. Mọi thao tác chạy trên dữ liệu cục bộ và hoạt động đầy đủ khi offline. Các máy đồng bộ qua một server do chính người dùng triển khai trên Cloudflare, và server chỉ giữ dữ liệu đã mã hoá.

## 2. Khác Notion ở đâu

| Chủ đề | Note Everywhere |
|---|---|
| Nơi dữ liệu sống | Trên máy trước, server chỉ để đồng bộ. Không có trạng thái "đang tải trang". |
| Quyền riêng tư | Mã hoá đầu-cuối. Server không đọc được nội dung. |
| Người dùng | Một người, nhiều máy. Không có chia sẻ, phân quyền, bình luận hay cộng tác cùng lúc. |
| Secret | Có vault riêng, mở bằng mật khẩu cấp 2. |
| Giọng nói | Push-to-Talk chuyển thành văn bản ngay trên máy (có điều kiện, xem mục 9). |
| Nhận diện | Giao diện, icon, tên gọi và bố cục riêng. Không sao chép của Notion. |

## 3. Mô hình nội dung

- Mọi nội dung là block. Mỗi block có id, kiểu, dữ liệu theo kiểu, danh sách block con và đúng một cha (CM-01 đến CM-03).
- Trang cũng là một node trong cây workspace và có thể lồng trong trang khác không giới hạn độ sâu.
- Thư mục là một node chỉ để chứa, không có nội dung.
- Đổi kiểu block giữ lại dữ liệu cũ; đổi to-do sang heading rồi đổi lại vẫn còn trạng thái đã đánh dấu (CM-04).
- Mỗi trang chỉ có một cha. Block "liên kết tới trang" là một liên kết, không tạo thêm quan hệ cha con (đề xuất Q3).

## 4. Block trong v1

Bắt buộc (15 kiểu): đoạn văn; heading cấp 1, 2, 3; danh sách gạch đầu dòng; danh sách đánh số; to-do; toggle; quote; callout; divider; code; ảnh; trang con; liên kết tới trang (BLK-01 đến BLK-13).

Mọi block đều di chuyển, nhân bản, xoá và đổi kiểu được (BLK-14, BLK-15).

Nên có nếu kịp: file đính kèm và bảng đơn giản. Hai kiểu này có trong plan ban đầu nhưng không nằm trong 15 kiểu bắt buộc của bản kiểm kê; thứ tự ưu tiên này là đề xuất của người viết spec.

## 5. Soạn thảo

- Gõ `/` mở menu chèn block, lọc theo chữ đang gõ (SLA-01 đến SLA-05).
- Markdown shortcut khi gõ: `#`, `##`, `###`, `-`, `1.`, `[]`, `---`, ba dấu backtick, và định dạng inline đậm, nghiêng, code, gạch ngang.
- Dấu `>` đầu dòng tạo **quote** theo Markdown chuẩn, không tạo toggle như Notion, để file Markdown bên ngoài nhập vào đúng nghĩa (đề xuất Q2).
- Enter tạo block mới, Shift+Enter xuống dòng trong block, Tab và Shift+Tab đổi cấp lồng.
- Phím tắt tối thiểu theo nhóm KEY trong bản kiểm kê; dùng Cmd trên macOS và Ctrl trên Windows.
- Undo và redo theo từng thiết bị; không hoàn tác chỉnh sửa đến từ máy khác.

## 6. Tổ chức

- Sidebar hiện cây thư mục và trang, mở gập từng nhánh (TRE-01 đến TRE-04).
- Kéo-thả để sắp xếp lại và chuyển cha, cho cả trang lẫn block (nhóm DND).
- Không cho kéo một trang vào chính con cháu của nó. Nếu hai máy tạo ra tình huống đó khi offline, đúng một lệnh di chuyển được giữ và không bao giờ có vòng lặp. Hành vi này đã có test ở `crates/ne-core/tests/crdt_convergence_spike.rs`.
- Yêu thích: ghim trang lên đầu sidebar.
- Thùng rác: xoá trang là xoá mềm kèm toàn bộ con cháu; khôi phục đưa trang về chỗ cũ, hoặc về gốc nếu cha không còn. Trang ở trong thùng rác cho tới khi người dùng xoá hẳn; tự dọn theo thời gian để sau v1 (đề xuất Q4).

## 7. Tìm kiếm và liên kết

- Tìm toàn văn trên tiêu đề và nội dung, mở bằng Cmd/Ctrl+K; tìm trong trang bằng Cmd/Ctrl+F (nhóm SRC).
- Tìm không phân biệt dấu tiếng Việt: gõ "ghi chu" khớp "ghi chú". Notion không công bố hành vi này; đây là yêu cầu riêng của Note Everywhere.
- Mục tiêu dưới 50 ms trên 10.000 trang. Con số này là mục tiêu của plan, chưa đo.
- Nội dung vault không bao giờ xuất hiện trong kết quả tìm kiếm.
- Gõ `[[` hoặc `@` để chèn liên kết tới trang khác. Mỗi trang có danh sách backlink (LNK-01 đến LNK-04).
- Liên kết tới trang đã vào thùng rác hiện ở dạng mờ và không mở được; khôi phục trang thì liên kết sống lại (LNK-08, cần kiểm chứng cách Notion xử lý).

## 8. Đồng bộ, tài khoản và vault

- Sửa được mọi thứ khi offline. Khi có mạng, thay đổi tự gửi đi và các máy hội tụ về cùng nội dung (nhóm OFF).
- Hai máy cùng sửa một đoạn văn: giữ chỉnh sửa của cả hai, gộp theo ký tự. Không có hộp thoại "chọn phiên bản".
- Thanh trạng thái cho biết: đã đồng bộ, đang chờ gửi, offline, hoặc lỗi.
- Đăng nhập bằng tài khoản và mật khẩu. Mật khẩu không rời máy. Recovery key được tạo lúc lập tài khoản, vì server không thể đặt lại mật khẩu.
- Vault lưu đăng nhập, ghi chú bảo mật và API key; mở bằng mật khẩu cấp 2; tự khoá khi máy idle, sleep hoặc khoá màn hình.

## 9. Push-to-Talk (có điều kiện)

Giữ phím nóng để nói, thả ra thì văn bản được chèn tại con trỏ. Nhận dạng tiếng Việt và tiếng Anh, chạy hoàn toàn trên máy. Tính năng chỉ được làm nếu spike đạt ngưỡng: sai số từ tiếng Việt dưới 12% và 10 giây lời nói xử lý dưới 5 giây trên máy không có GPU rời. Không đạt thì bỏ.

## 10. Nhập và xuất

- Xuất một trang hoặc cả nhánh ra Markdown theo CommonMark và GFM. Block không có dạng tương đương (callout, toggle) xuất ra dạng đọc được kèm chú thích (đề xuất Q1, phương án A).
- Nếu kịp: kèm một file JSON để nhập lại vào Note Everywhere không mất cấu trúc (phương án B).
- Nhập thư mục Markdown, giữ cấu trúc thư mục thành cây trang và giữ file ảnh đi kèm.
- Tên file xuất phải hợp lệ trên Windows (EXP-09).

## 11. Sau v1

Database và các view, template, lịch sử trang, block đồng bộ, nhắc việc, nhập từ bản xuất của Notion, tự dọn thùng rác, tô màu cú pháp trong code block, heading gập được.

## 12. Ngoài phạm vi

Mobile, web, Linux, nhiều người dùng, chia sẻ công khai, bình luận, AI assistant, API công khai.

## 13. Đề xuất chờ xác nhận

Bốn lựa chọn dưới đây lấy từ mục 17 của bản kiểm kê và đã được viết vào spec như mặc định. Đổi bất kỳ mục nào thì sửa phần tương ứng ở trên.

1. Markdown theo CommonMark và GFM, không tạo cú pháp riêng.
2. `>` là quote, không phải toggle.
3. Liên kết tới trang không tạo quan hệ cha con.
4. Thùng rác giữ trang cho tới khi người dùng xoá hẳn.
