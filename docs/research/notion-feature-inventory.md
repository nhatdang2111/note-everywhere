# Kiểm kê tính năng Notion (Phase 1)

Ngày lập: 2026-11-21. Nguồn: chỉ tài liệu công khai (xem [public-sources.md](./public-sources.md)). Không dùng app, không phân tích gói cài đặt, không bắt lưu lượng.

## 0. Cách đọc

- **Loại** cho biết căn cứ của mỗi mục:
  - *Tài liệu*: Notion công bố trực tiếp trong nguồn đã ghi.
  - *Suy luận*: tôi rút ra từ tài liệu, không có nguồn nào nói thẳng.
  - *Chưa rõ*: nguồn công khai không trả lời. Các mục quan trọng nhất có trong [hands-on-checklist.md](./hands-on-checklist.md), các mục còn lại cần kiểm chứng sau.
- **Ưu tiên** cho Note Everywhere (NE):
  - *Must*: bắt buộc cho v1, theo kế hoạch nội bộ (P01, không công bố trong repo).
  - *Should*: v1.1 hoặc ngay sau v1.
  - *Later*: chưa làm, không chặn v1.
- **Hạn chế của lần nghiên cứu này**:
  - Không có quan sát bằng tay. Mọi hành vi đều lấy từ tài liệu, nên có thể lệch với bản app hiện tại.
  - Blog kỹ thuật đọc được có bốn bài. Bài mô hình block (2021) là bài cũ nhất và chỉ dùng cho khái niệm.
  - Nhiều bài trợ giúp có nội dung AI, Meetings, Home, Library. Các phần đó nằm ngoài phạm vi v1 và không được đưa vào inventory.
  - Không khảo sát mobile và web, vì NE chỉ có desktop.

## 1. Mô hình khái niệm

Mọi khẳng định trong nhóm này đều có nguồn công khai. Đây là nền để các nhóm sau hiểu nhất quán.

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| CM-01 | Mọi nội dung trong Notion là block. Đoạn văn, ảnh, dòng database và cả page đều là block. | Page cũng là block, nên page có thể lồng trong page. | Tài liệu | S02, S36 | Must |
| CM-02 | Mỗi block có id duy nhất, một type, một phần dữ liệu riêng theo type, danh sách block con và một parent. | API mô tả cùng cấu trúc: id, parent, type, has_children, in_trash và payload theo type. | Tài liệu | S36, S30 | Must |
| CM-03 | Parent là con trỏ ngược, dùng cho quyền truy cập. Danh sách con là con trỏ xuôi, dùng để hiển thị. | Notion từng cho một block có nhiều tham chiếu cha, rồi bỏ vì quyền không còn xác định được. | Tài liệu | S36 | Must |
| CM-04 | Đổi type của block không xóa dữ liệu cũ. Ví dụ đổi to-do sang heading rồi quay lại vẫn giữ trạng thái đã check. | Mô tả trong bài 2021. Chưa kiểm chứng với bản hiện tại. | Tài liệu | S36 | Must |
| CM-05 | Page con là block con của page cha. Trong sidebar, lồng không giới hạn về mặt tài liệu. | Không có giới hạn độ sâu được công bố. Blog 2021 cho biết cây block có độ sâu và độ rộng thay đổi rất lớn. | Tài liệu | S06, S08, S38 | Must |
| CM-06 | Cách hiển thị con phụ thuộc type. Danh sách thụt lề con. Toggle chỉ hiện con khi mở. Page mở con ở trang riêng. | Đây là lý do cùng một quan hệ cha con có thể hiện theo ba cách khác nhau. | Tài liệu | S36, S04 | Must |
| CM-07 | Database là một page. Mỗi hàng (row) là một page có property. | Hàng mở được như page và có nội dung riêng. | Tài liệu | S17, S21 | Should |
| CM-08 | Database chứa một hoặc nhiều data source. Data source là tập page. Giá trị property của page phải khớp schema của data source. | Một database có thể liên kết data source từ database khác. Mô hình này được API đưa vào từ phiên bản 2025-09-03. | Tài liệu | S18, S32, S31, S34 | Should |
| CM-09 | Nội dung page là danh sách block con. API đọc theo trang, tối đa 100 mục mỗi trang. | Đây là chi tiết API, không phải hành vi người dùng. | Tài liệu | S33 | Must |
| CM-10 | API giữ các kiểu block chưa hỗ trợ dưới dạng "unsupported" thay vì bỏ qua. | Suy luận: NE nên giữ block type chưa biết khi đọc dữ liệu mới hơn, để tránh mất nội dung khi nâng cấp. | Tài liệu / Suy luận | S30 | Must |
| CM-11 | Thụt lề là quan hệ cấu trúc. Block được chuyển vào danh sách con của block anh em đứng trước. Nếu block đứng trước không phải kiểu list, thao tác thụt lề không có tác dụng. | Mô tả trong bài 2021. Quy tắc này có thể đã đổi. | Tài liệu | S36 | Must |
| CM-12 | Xóa mềm dùng cờ in_trash trên block và page. Page trong thùng rác không sửa được cho đến khi được khôi phục. | Cờ archived đã bị thay bằng in_trash trong API. | Tài liệu | S30, S31, S11 | Must |

## 2. Loại block

Danh sách 15 kiểu block Must là đề xuất của tôi, lấy từ nhóm block cơ bản của Notion và yêu cầu "15 loại block cơ bản" trong P01. Gồm: đoạn văn, ba cấp heading, danh sách gạch đầu dòng, danh sách đánh số, to-do, toggle, quote, callout, divider, code, image, child page, và link to page. Dòng BLK-02 gom ba cấp heading thành một mục.

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| BLK-01 | Đoạn văn. Enter tạo block mới. Shift+Enter xuống dòng trong cùng block. | Đây là block mặc định khi tạo page. | Tài liệu | S01, S04 | Must |
| BLK-02 | Heading cấp 1 đến 3. Tạo bằng markdown hoặc slash. | Heading có thể là toggle, xem BLK-17. | Tài liệu | S01, S26 | Must |
| BLK-03 | Danh sách gạch đầu dòng. Lồng bằng Tab. | Ký tự đầu dòng đều là markdown chuẩn. | Tài liệu | S01 | Must |
| BLK-04 | Danh sách đánh số kiểu 1. Các kiểu chữ cái và La Mã là tính năng riêng. | Đánh số tự tăng theo block anh em. Chi tiết tăng số khi lồng chưa rõ. | Tài liệu / Chưa rõ | S01, S04 | Must |
| BLK-05 | To-do có checkbox. Check bằng phím tắt hoặc bấm trực tiếp. | Cách đánh số lại khi xóa một to-do giữa chừng chưa rõ. | Tài liệu / Chưa rõ | S01, S04 | Must |
| BLK-06 | Toggle là block gập được, chứa block con. | Chỉ hiện tiêu đề khi gập. | Tài liệu | S36, S04 | Must |
| BLK-07 | Quote là khối trích dẫn. Tạo bằng dấu nháy kép đầu dòng. | Dấu ">" đầu dòng trong Notion tạo toggle, không phải quote. Đây là điểm khác markdown chuẩn cần quyết định khi nhập. | Tài liệu | S01 | Must |
| BLK-08 | Callout có icon và màu nền, chứa block con. | Khi xuất Markdown, callout không có dạng tương đương, xem EXP-02. | Tài liệu | S04, S16 | Must |
| BLK-09 | Divider là đường kẻ ngang. Tạo bằng ba dấu gạch ngang rồi Enter. | Không có chú thích kèm. | Tài liệu | S01 | Must |
| BLK-10 | Code block có chọn ngôn ngữ, copy và wrap dòng. | Tô màu cú pháp theo ngôn ngữ là tính năng riêng, xếp ở Should (xem BLK-22). Cách xử lý fence markdown bên trong code block chưa rõ. | Tài liệu / Chưa rõ | S28 | Must |
| BLK-11 | Image nhận file từ máy hoặc URL. Ảnh được hiển thị trong block. | Giới hạn dung lượng và cách lưu file trong NE chưa được quyết định. Mục URL là Later. | Tài liệu / Chưa rõ | S04, S03 | Must |
| BLK-12 | Child page: tạo page con ngay trong trang, hiện như một dòng trong cây. | Xem SLA-01 và TRE-02 để biết các cách tạo. | Tài liệu | S08 | Must |
| BLK-13 | Link to page: block là liên kết tới một page có sẵn. | Notion đưa page được liên kết vào sidebar như subpage. Suy luận: để giữ cây một cha, NE nên coi đây là liên kết, không phải con. | Tài liệu / Suy luận | S09 | Must |
| BLK-14 | Quy tắc chung: mọi block có thể đổi type, di chuyển, nhân bản và xóa. | Đổi kiểu giữ dữ liệu, xem CM-04. | Tài liệu | S04, S05 | Must |
| BLK-15 | Đổi kiểu qua menu của block. Chỉ áp dụng cho block chứa text; Notion cho phép đổi sang nhiều kiểu khác. | Ví dụ thường gặp: bullet sang quote, paragraph sang callout. | Tài liệu | S05, S02 | Should |
| BLK-16 | Nhân bản block đã chọn. | Phím tắt có trong KEY-10. | Tài liệu | S01 | Should |
| BLK-17 | Toggle đối với heading: heading có thể gập được. Mở và đóng thủ công. | Không có nút mở hoặc đóng tất cả. | Tài liệu | S26 | Later |
| BLK-18 | Bảng đơn giản (simple table): ô văn bản, gộp ô, header hàng và cột, chuyển sang database. | Gộp ô: ô trên cùng bên trái giữ nội dung. | Tài liệu | S26, S27 | Later |
| BLK-19 | Cột (column): kéo block cạnh nhau để tạo bố cục nhiều cột. Trên mobile các cột xếp dọc. | Cần ít nhất hai cột, mỗi cột có ít nhất một block (API). | Tài liệu | S26, S30 | Later |
| BLK-20 | Synced block: một block xuất hiện ở nhiều nơi và đồng bộ nội dung. | Xóa bản gốc khi có hơn 10 bản sao sẽ xóa hết các bản sao, và Undo không khôi phục. | Tài liệu | S25 | Later |
| BLK-21 | Mục lục (table of contents) tự lấy từ heading của trang. | Hiển thị như block hoặc ở lề phải khi cuộn. | Tài liệu | S26 | Should |
| BLK-22 | Tô màu cú pháp cho code block. | Phụ thuộc vào danh sách ngôn ngữ hỗ trợ, chưa công bố. | Tài liệu | S28 | Should |
| BLK-23 | Đổi màu chữ và màu nền của block. | Màu có ở cả text và background. | Tài liệu | S03 | Later |
| BLK-24 | Math equation, video, audio, PDF, file, embed, bookmark, link preview, template button, breadcrumb (ngoài mục 15). | Các block này phụ thuộc file, mạng hoặc nội dung ngoài. Không phù hợp v1 local-first. | Tài liệu | S04, S03 | Later |
| BLK-25 | Breadcrumb hiện đường dẫn tới page hiện tại ở đầu trang. | Đây là chỉ báo vị trí, không phải block. | Tài liệu | S08 | Should |

## 3. Slash menu và markdown shortcut

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| SLA-01 | Gõ "/" ở đầu hoặc cuối block để mở menu chèn block. Gõ tiếp để lọc theo tên. | Esc đóng menu và cho phép gõ "/" như ký tự thường. | Tài liệu | S01, S03 | Must |
| SLA-02 | Menu slash chia theo nhóm: cơ bản, inline, media, nâng cao. | Phần lớn nhóm media và nâng cao là Later. | Tài liệu | S03, S04 | Must |
| SLA-03 | Gõ markdown ở đầu dòng rồi Space để tạo block: gạch đầu dòng, to-do, số, heading, quote, toggle, divider. | Ký tự ">" tạo toggle và ký tự '"' tạo quote. NE cần quyết định dùng quy ước nào, xem mục 17. | Tài liệu | S01 | Must |
| SLA-04 | Markdown inline: hai dấu sao cho đậm, một dấu sao cho nghiêng, dấu backtick cho code, dấu ngã đôi cho gạch ngang. | Dấu sao đầu dòng là bullet, dấu sao giữa dòng là nghiêng. | Tài liệu | S01 | Must |
| SLA-05 | Gõ "@" rồi tên trang để tạo liên kết inline tới trang. | Gõ "@" rồi ngày sẽ tạo mention ngày, xếp Later. | Tài liệu | S01, S09 | Must |
| SLA-06 | Gõ "[[" hoặc "+" rồi tên trang để liên kết. "+" và "[[" khác nhau ở thứ tự gợi ý: "[[" ưu tiên liên kết, "+" ưu tiên tạo trang. | Hai cách cho cùng kết quả khi chọn liên kết. | Tài liệu | S01, S09 | Should |
| SLA-07 | Tạo trang mới ngay từ "[[" hoặc "+" và đặt nó dưới trang hiện tại hoặc ở nơi khác. | Trang được tạo ở nơi người dùng chọn trong dropdown. | Tài liệu | S09, S01 | Later |
| SLA-08 | Nhắc nhở bằng lệnh riêng kèm ngày, và gõ "@" kèm ngày tương đối. | Nhắc nhở cần thông báo, không nằm trong v1. | Tài liệu | S01 | Later |
| SLA-09 | Emoji gõ ":tên" rồi chọn. | Chỉ là chức năng gợi ý nhanh. | Tài liệu | S01 | Later |
| SLA-10 | Markdown gõ xong rồi Enter không tự đổi kiểu trong một số trường hợp. Hành vi chính xác chưa rõ. | Cần kiểm tra thủ công. | Chưa rõ | S01 | Must |

## 4. Phím tắt

Trên Windows, Notion ghi chú rằng ctrl+shift có thể bị hệ điều hành chiếm, nên NE phải kiểm tra xung đột trên Windows trước khi chốt phím tắt.

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| KEY-01 | Cmd/Ctrl+P hoặc Cmd/Ctrl+K mở tìm kiếm và trang gần đây. Cmd/Ctrl+K thêm liên kết khi có chữ đang được chọn. | Hai chức năng cùng một phím, phụ thuộc trạng thái con trỏ. | Tài liệu | S01, S10 | Must |
| KEY-02 | Cmd/Ctrl+F tìm trong trang đang mở. | Không tìm trong các trang khác. | Tài liệu | S01, S10 | Must |
| KEY-03 | Cmd/Ctrl+\ ẩn hoặc hiện sidebar. | Không có chú thích. | Tài liệu | S06 | Must |
| KEY-04 | Cmd/Ctrl+N tạo page mới. | Trang mới mở ngay. | Tài liệu | S01 | Must |
| KEY-05 | Cmd/Ctrl+[ và ] quay lại và tiến trong lịch sử điều hướng. | Không rõ có lưu lịch sử qua phiên làm việc không. | Tài liệu / Chưa rõ | S01 | Should |
| KEY-06 | Cmd/Ctrl+B, I, E, K định dạng đậm, nghiêng, code inline và liên kết cho chữ đang chọn. | Cmd/Ctrl+K với chữ được chọn là thêm liên kết, không phải tìm kiếm. | Tài liệu | S01 | Must |
| KEY-07 | Cmd/Ctrl+U gạch chân và Cmd/Ctrl+Shift+S gạch ngang. | Gạch chân không có trong markdown chuẩn. | Tài liệu | S01 | Should |
| KEY-08 | Tab và Shift+Tab thụt lề hoặc gỡ lề block. Thụt lề đưa block vào block trước đó. | Quy tắc chỉ áp dụng khi block trước là list (CM-11). | Tài liệu | S01, S36 | Must |
| KEY-09 | Esc chọn block đang có con trỏ. Cmd/Ctrl+A chọn block hiện tại. Shift+click chọn cả dải. | Chọn nhiều block bằng Shift+mũi tên là Should (KEY-10). | Tài liệu | S01 | Must |
| KEY-10 | Shift+mũi tên mở rộng vùng chọn lên hoặc xuống. | Không có chú thích. | Tài liệu | S01 | Should |
| KEY-11 | Backspace hoặc Delete xóa block đã chọn. | Xóa đưa block vào thùng rác của block (không phải page). Hành vi chi tiết chưa rõ. | Tài liệu / Chưa rõ | S01, S11 | Must |
| KEY-12 | Cmd/Ctrl+D nhân bản block đã chọn. | Alt khi kéo cũng nhân bản, xem DND-08. | Tài liệu | S01 | Should |
| KEY-13 | Cmd/Ctrl+Shift+mũi tên di chuyển block đã chọn lên hoặc xuống. | Không có chú thích. | Tài liệu | S01 | Must |
| KEY-14 | Cmd/Ctrl+/ mở menu để đổi kiểu, màu hoặc thao tác với một hoặc nhiều block. | Trong database, dùng để sửa hàng loạt. | Tài liệu | S01, S27 | Should |
| KEY-15 | Shift+Enter xuống dòng trong block. | Khác với Enter, vốn tạo block mới. | Tài liệu | S01 | Must |
| KEY-16 | Cmd+Option (Mac) hoặc Ctrl+Shift (Windows, Linux) cộng số từ 0 đến 9 tạo nhanh kiểu block. | Ctrl+Shift trên Windows có thể bị hệ điều hành chiếm. | Tài liệu | S01 | Later |
| KEY-17 | Cmd/Ctrl+Shift+L chuyển giữa sáng và tối. | Chủ đề là phần của giao diện. | Tài liệu | S01 | Should |
| KEY-18 | Cmd/Ctrl+Shift+N mở cửa sổ mới. Cmd/Ctrl+T tạo tab mới. | Cần quyết định NE có nhiều cửa sổ hay không. | Tài liệu | S01 | Later |
| KEY-19 | Cmd/Ctrl+Click mở liên kết trong tab mới. | Tab là tính năng riêng. | Tài liệu | S01, S10 | Later |
| KEY-20 | Cmd/Ctrl+L sao chép URL của trang. | NE chưa có URL public. Có thể thay bằng liên kết nội bộ. | Tài liệu | S01 | Later |

## 5. Kéo thả

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| DND-01 | Kéo tay cầm ⋮⋮ bên trái block để đổi vị trí trong trang. Đường chỉ dẫn hiện khi kéo. | Tay cầm xuất hiện khi hover. | Tài liệu | S02, S03 | Must |
| DND-02 | Kéo block cạnh block khác để tạo cột mới. | Cột là Later (BLK-19). | Tài liệu | S26 | Later |
| DND-03 | Kéo cột về lại hàng để gỡ cột. Cột trống xóa bằng menu Delete. | Cột trên mobile xếp dọc. | Tài liệu | S26 | Later |
| DND-04 | Kéo page trong sidebar vào page khác để lồng nó làm con. | Page đích được tô sáng khi kéo. | Tài liệu | S06, S08 | Must |
| DND-05 | Kéo page ra khỏi toggle cha để tách nó khỏi cha. | Có thể thả vào vị trí cùng cấp hoặc chỗ khác. | Tài liệu | S08, S06 | Must |
| DND-06 | Kéo page vào thùng rác. Các subpage đi theo. | Cùng hành vi với TRA-01. | Tài liệu | S06, S11 | Must |
| DND-07 | Kéo page từ khu chia sẻ vào khu riêng tư làm người khác mất quyền. | Không áp dụng cho v1 (một người dùng). Bài học: thao tác kéo có thể thay đổi quyền. | Tài liệu | S06 | Later |
| DND-08 | Kéo khi giữ Alt tạo bản sao block. | Không có chú thích. | Tài liệu | S01 | Later |
| DND-09 | Kéo page vào chính nó hoặc vào một con của nó. Notion không nói rõ hành vi. | Ảnh hưởng trực tiếp tới cây. Suy luận: NE phải từ chối tạo chu trình (plan P01, movable tree). | Chưa rõ / Suy luận | (không có nguồn công khai) | Must |
| DND-10 | Kéo block có con thì con có đi theo không. | Notion mô tả con là một phần của block cha (CM-02), nhưng không nói rõ khi kéo. | Chưa rõ | S36 | Must |
| DND-11 | Chuyển block sang page khác bằng chức năng riêng. | Không cần kéo sang page khác cho v1. | Tài liệu | S04 | Later |
| DND-12 | Kéo để đổi thứ tự sort, option select, property và view tab trong database. | Chỉ có trong database. | Tài liệu | S20, S19 | Should |

## 6. Sidebar và cây trang

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| TRE-01 | Sidebar hiện cây page lồng nhau. Mỗi page có mũi tên để mở hoặc đóng con. | Subpage có chức năng giống page cha. | Tài liệu | S06 | Must |
| TRE-02 | Hover một page rồi bấm "+" để tạo subpage ngay trong page đó. | Có "+" cạnh tên mỗi mục ở sidebar. | Tài liệu | S08 | Must |
| TRE-03 | Page mới tạo từ sidebar mặc định nằm trong khu riêng tư của người dùng. | NE chỉ có một người dùng, nên khu riêng tư là vùng duy nhất. | Tài liệu / Suy luận | S06 | Must |
| TRE-04 | Thu gọn hoặc mở rộng sidebar bằng nút và phím tắt. | Phím tắt là KEY-03. | Tài liệu | S06 | Must |
| TRE-05 | Đổi kích thước sidebar bằng cách kéo cạnh phải. | Không có giới hạn kích thước được công bố. | Tài liệu | S06 | Should |
| TRE-06 | Sắp xếp page con theo thủ công hoặc theo thời gian sửa gần nhất. | Sắp xếp thủ công và theo thời gian là hai chế độ riêng. | Tài liệu | S06 | Should |
| TRE-07 | Giới hạn số page hiện trong sidebar, có mục nút xem thêm mở danh sách đầy đủ. | Số lượng hiện có thể chọn từ 5 đến tất cả. | Tài liệu | S06 | Should |
| TRE-08 | Ẩn hoặc hiện từng section và từng shortcut trong sidebar. Có thể đặt lại về mặc định. | Thiết lập sidebar không ảnh hưởng nội dung. | Tài liệu | S07 | Later |
| TRE-09 | Nhiều tab trong sidebar, mỗi tab chứa shortcut và section riêng. | Tab và preset là lớp cấu hình riêng, Notion có tối đa sáu tab. | Tài liệu | S07 | Later |
| TRE-10 | Shortcut tới một page trong sidebar mà không cần đưa vào yêu thích. | Xem FAV-04. | Tài liệu | S07 | Should |
| TRE-11 | Ghim một view database vào sidebar. | Phụ thuộc database. | Tài liệu | S07 | Later |
| TRE-12 | Sidebar đã thiết lập chỉ thuộc một người, không ảnh hưởng người khác. | Không cần cho NE v1. | Tài liệu | S07 | Later |

## 7. Yêu thích

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| FAV-01 | Bấm biểu tượng sao ở đầu trang để đưa trang vào mục yêu thích. Trang xuất hiện trong sidebar. | Mục yêu thích chỉ hiện sau khi có mục đầu tiên. | Tài liệu | S06 | Should |
| FAV-02 | Bỏ yêu thích bằng cách bấm lại sao ở đầu trang hoặc menu của trang trong sidebar. | Không có chú thích. | Tài liệu | S06 | Should |
| FAV-03 | Mục yêu thích là một khu riêng trong sidebar, tách khỏi cây trang. | Nguồn không nói trang yêu thích có còn nằm trong cây hay không. Suy luận: có, vì yêu thích là một cách hiển thị thêm. | Tài liệu / Suy luận | S06 | Should |
| FAV-04 | Shortcut trong một tab là lựa chọn nhẹ hơn yêu thích. | Shortcut chỉ có trong tab đó. | Tài liệu | S06, S07 | Should |
| FAV-05 | Thứ tự các mục yêu thích có thể kéo đổi hay không. | Chưa rõ. | Chưa rõ | S06 | Later |

## 8. Thùng rác và lưu trữ

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| TRA-01 | Đưa page vào thùng rác bằng menu của trang, kéo vào thùng rác, hoặc nhấn Backspace/Delete trên page block. Subpage đi theo. | Đây là đường dẫn chính để xóa trang. | Tài liệu | S06, S11 | Must |
| TRA-02 | Mở thùng rác để xem, tìm và khôi phục page. | Nguồn không nói rõ vị trí trang sau khi khôi phục, và hành vi khi trang cha đã bị xóa cũng chưa rõ. | Tài liệu / Chưa rõ | S11 | Must |
| TRA-03 | Trang trong thùng rác không sửa được cho đến khi khôi phục. | Có thể đọc được. | Tài liệu | S06, S11 | Must |
| TRA-04 | Trang trong thùng rác giữ mặc định 30 ngày, sau đó chuyển sang trạng thái đã xóa và giữ thêm 30 ngày, rồi xóa vĩnh viễn. | Xóa vĩnh viễn không khôi phục được. | Tài liệu | S11 | Should |
| TRA-05 | Thùng rác có bộ lọc theo người sửa cuối và vị trí cũ. | Bộ lọc là phần mở rộng của TRA-02. | Tài liệu | S11 | Should |
| TRA-06 | Không có nút làm rỗng thùng rác một lần. Phải xóa từng trang hoặc chờ hết hạn. | NE có thể bổ sung nút này, nhưng đó là quyết định sản phẩm. | Tài liệu | S11 | Later |
| TRA-07 | Undo sau khi xóa block hoặc page. Phạm vi undo chưa được mô tả. | Không rõ undo có quay lại trạng thái trước nhiều bước không. | Chưa rõ | S25, S11 | Must |
| TRA-08 | Lưu trữ (archive) là trạng thái riêng, khác thùng rác. Trang lưu trữ vẫn mở được, liên kết vẫn hoạt động, có banner. | Có ở gói trả phí, đang beta. | Tài liệu | S12 | Later |
| TRA-09 | Lưu trữ trang cha làm lưu trữ tất cả con. | Chỉ áp dụng khi có tính năng lưu trữ. | Tài liệu | S12 | Later |
| TRA-10 | Lưu trữ hàng trong database làm hàng biến mất khỏi view, và xem được ở phần cài đặt database. | Phụ thuộc database. | Tài liệu | S12 | Later |

## 9. Tìm kiếm

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| SRC-01 | Mở hộp tìm kiếm toàn workspace bằng Cmd/Ctrl+P, Cmd/Ctrl+K hoặc nút trên sidebar. Khi chưa gõ, hộp hiện các trang đã xem gần đây. | Phím tắt chồng với KEY-06 khi có chữ được chọn. | Tài liệu | S10 | Must |
| SRC-02 | Tìm chính xác cụm từ bằng cách đặt trong dấu ngoặc kép. | Không có chú thích về tiếng Việt. | Tài liệu | S10 | Should |
| SRC-03 | Mặc định xếp kết quả theo độ khớp. Trang sửa gần đây được ưu tiên. Khớp tiêu đề được ưu tiên hơn khớp nội dung. | Trọng số cụ thể không được công bố. | Tài liệu | S10 | Must |
| SRC-04 | Bộ lọc gồm: chỉ tiêu đề, người tạo, khoảng thời gian, trong một trang và các subpage của nó. Sắp xếp theo thời gian sửa hoặc tạo. | Bộ lọc thời gian có các mốc: hôm nay, 7 ngày, 30 ngày, hoặc khoảng tùy chọn. | Tài liệu | S10 | Should |
| SRC-05 | Tìm trong trang bằng Cmd/Ctrl+F. | Chỉ trong trang đang mở. | Tài liệu | S10 | Must |
| SRC-06 | Tìm trong database lọc hàng theo tiêu đề và giá trị property. Không tìm trong nội dung trang. | Database cần tối thiểu ba trang để hiện thanh tìm. | Tài liệu | S10, S20 | Should |
| SRC-07 | Tìm toàn workspace không tìm trong comment, giá trị select và multi-select, và mention người hay trang. | Đây là giới hạn cần biết khi chọn phạm vi chỉ mục của NE. | Tài liệu | S10 | Must |
| SRC-08 | Trang trong thùng rác không hiện trong tìm kiếm chung. | Thùng rác có tìm kiếm riêng (TRA-02). | Tài liệu / Suy luận | S11 | Must |
| SRC-09 | Trang lưu trữ bị ẩn khỏi tìm kiếm mặc định. | Chỉ áp dụng khi có lưu trữ. | Tài liệu | S12 | Later |
| SRC-10 | Tìm không dấu tiếng Việt, ví dụ gõ "ban" tìm được "bản". | Tài liệu không nói rõ. Plan P01 yêu cầu việc này. | Chưa rõ | (không có nguồn công khai) | Must |
| SRC-11 | Thời gian phản hồi tìm kiếm. | Notion không công bố số liệu. P01 đặt mục tiêu dưới 50 ms cho 10.000 trang, đây là mục tiêu của NE chưa kiểm chứng. | Chưa rõ | S10, P01 | Must |
| SRC-12 | Mỗi thay đổi block được đưa vào chỉ mục, và quyền truy cập được gắn vào từng tài liệu tìm kiếm. | Notion dùng Elasticsearch cho tìm từ khóa. NE chưa cần quyền khi chỉ có một người dùng. | Tài liệu / Suy luận | S39 | Must |
| SRC-13 | Tìm bằng ngôn ngữ tự nhiên hoặc theo ngữ nghĩa. | Tính năng AI. Nằm ngoài v1 (P01 loại trừ trợ lý AI). | Tài liệu | S10 | Later |

## 10. Liên kết và backlink

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| LNK-01 | Gõ "@", "[[" hoặc "+" rồi tên trang để tạo liên kết inline tới trang. Đổi tên trang thì tên hiển thị trong liên kết tự đổi theo. | Liên kết được lưu theo định danh trang, không theo tên. Suy luận từ hành vi đổi tên. | Tài liệu / Suy luận | S09, S10 | Must |
| LNK-02 | Liên kết inline không làm trang đích thành subpage trong sidebar. | Khác với BLK-13. | Tài liệu | S09 | Must |
| LNK-03 | Backlink được tạo tự động khi có mention. Hiện phía trên tiêu đề trang khi có. | Ẩn khi không có backlink. | Tài liệu | S09 | Must |
| LNK-04 | Backlink chỉ hiện nguồn mà người xem có quyền. Nguồn riêng tư được ghi nhãn riêng tư. | Với một người dùng, phần quyền là không cần. | Tài liệu | S09 | Must |
| LNK-05 | Anchor link tới từng block: sao chép link của block rồi dán ở nơi khác. | Cần ID block bền vững. | Tài liệu | S09 | Later |
| LNK-06 | Dán URL web thành link mention có icon, nguồn và tiêu đề. Hover để xem trước. | Cần mạng, không hợp với local-first ở v1. | Tài liệu | S09 | Later |
| LNK-07 | Mở link Notion trong app desktop. | Không áp dụng cho NE. | Tài liệu | S09 | Later |
| LNK-08 | Xóa một trang đang được liên kết thì các liên kết còn lại hiện thế nào. | Notion không nói rõ. | Chưa rõ | (không có nguồn công khai) | Must |
| LNK-09 | Backlink trong database hiện ở phần thuộc tính của page. | Phụ thuộc database. | Tài liệu | S09 | Should |
| LNK-10 | Notion có tính năng chia sẻ page ra web công khai, là tính năng riêng khỏi liên kết nội bộ. | P01 loại trừ chia sẻ công khai khỏi v1. | Tài liệu / P01 | S29, P01 | Later |

## 11. Database

Nhóm này có ưu tiên Should, theo P01 (phase 8, P2). Các phần relation, formula và rollup là Later.

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| DB-01 | Database là page chứa các page con là hàng. Mỗi hàng mở được như page và có nội dung riêng. | Database có thể nằm trong page hoặc là page riêng. | Tài liệu | S17, S21 | Should |
| DB-02 | Property kiểu văn bản, số, select, multi-select, status, ngày, checkbox, URL, và các mốc thời gian tạo và sửa. | Các property tự động không sửa được. | Tài liệu | S19 | Should |
| DB-03 | Property nâng cao: formula, relation, rollup, person, file, button, place, ID. | Phụ thuộc nhiều tính năng khác. | Tài liệu | S19, S22 | Later |
| DB-04 | Giới hạn 500 property cho mỗi database. | Khi đạt giới hạn, property mới không được tạo. | Tài liệu | S19 | Should |
| DB-05 | Thêm option cho select, màu được gán ngẫu nhiên, sắp xếp option bằng cách kéo. | Đổi tên hoặc xóa option thực hiện từ ô. | Tài liệu | S19, S20 | Should |
| DB-06 | Property ngày có khoảng (end date), có giờ, có múi giờ. Nhắc nhở là Later. | Múi giờ theo cài đặt của property. | Tài liệu | S19 | Should |
| DB-07 | Các kiểu view: table, board, list, gallery, timeline, calendar, chart, form. | Form và chart nằm ngoài v1. | Tài liệu | S20 | Should |
| DB-08 | Mỗi view có filter, sort, group và property visibility riêng. Thay đổi chỉ tác động view đó. | Khác với data source dùng chung. | Tài liệu | S20 | Should |
| DB-09 | Filter một điều kiện, hoặc nhóm AND/OR lồng tối đa ba tầng. | Nhóm nâng cao là Later. | Tài liệu | S20 | Should |
| DB-10 | Sort nhiều cấp, đổi thứ tự cấp bằng cách kéo. Text sắp theo bảng chữ cái, số theo giá trị, select theo thứ tự option. | Thứ tự sắp xếp của tiếng Việt có dấu chưa được công bố. | Tài liệu / Chưa rõ | S20 | Should |
| DB-11 | Group theo property, ẩn nhóm, sắp xếp nhóm, ẩn nhóm rỗng, và sub-group. | Group là Later. | Tài liệu | S20 | Later |
| DB-12 | Mở hàng theo kiểu hiện bên phải, hiện giữa màn hình, hoặc mở toàn trang, chọn riêng cho từng view. | Hiện bên phải hoặc mở toàn trang là lựa chọn đề xuất cho v1. | Tài liệu | S20 | Should |
| DB-13 | Chọn filter hoặc sort áp dụng cho mọi người hay chỉ cho mình. | Không cần khi chỉ có một người dùng. | Tài liệu / Suy luận | S20 | Later |
| DB-14 | Cố định một cột bên trái khi cuộn ngang. | Không có chú thích. | Tài liệu | S20 | Later |
| DB-15 | Sửa hàng loạt các hàng đã chọn. | Phụ thuộc Cmd/Ctrl+/ (KEY-14). | Tài liệu | S27 | Later |
| DB-16 | Calculation ở chân cột: đếm, đếm giá trị duy nhất, tổng, trung bình, trung vị, min, max. | Tùy kiểu property. | Tài liệu | S27 | Later |
| DB-17 | Relation một chiều mặc định. Có thể bật hai chiều. Giới hạn số liên kết là 1 hoặc không giới hạn. Xóa liên kết không xóa page. | Relation hai chiều tạo hai property. | Tài liệu | S22 | Later |
| DB-18 | Rollup tổng hợp giá trị qua relation. | Phụ thuộc DB-17. | Tài liệu | S22 | Later |
| DB-19 | Conditional color đổi màu nền hàng theo điều kiện, cấu hình theo view. | Chỉ áp dụng cho page, không tô một ô. | Tài liệu | S19 | Later |
| DB-20 | Một database có nhiều data source, và có thể liên kết data source của database khác. | Chỉnh sửa view không ảnh hưởng database gốc, nhưng chỉnh property và tiêu đề thì ảnh hưởng. | Tài liệu | S18 | Later |
| DB-21 | Xóa property đưa nó vào danh sách property đã xóa, có thể khôi phục hoặc xóa vĩnh viễn. | Xóa vĩnh viễn không khôi phục được. | Tài liệu | S11 | Later |
| DB-22 | Comment trên giá trị property. | Không có trong v1. | Tài liệu | S19 | Later |

## 12. Template

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| TPL-01 | Nhân bản page như cách làm template thủ công. Giữ một page trống và nhân bản mỗi khi cần. | Đây là cách Notion khuyến nghị cho page template. | Tài liệu | S24 | Should |
| TPL-02 | Notion có thư viện template và Marketplace để thêm template vào workspace. | Chi tiết về giá và điều kiện thêm template không nằm trong phạm vi đã đọc. | Tài liệu | S24 | Later |
| TPL-03 | Nút template nhân bản một nhóm block khi bấm. | Có ở mục nâng cao. | Tài liệu | S03, S04 | Later |
| TPL-04 | Database template gắn với một database, có thể đặt giá trị property mặc định và nội dung sẵn. | Chỉ dùng trong database đó. | Tài liệu | S23 | Later |
| TPL-05 | Database template có thể lặp lại theo ngày, tuần, tháng hoặc năm. | Template lặp hằng ngày không được lồng template khác. | Tài liệu | S23 | Later |
| TPL-06 | Không nên điền relation sẵn trong template, trừ khi muốn mọi trang tạo ra đều liên kết cùng một đích. | Chỉ áp dụng khi có relation. | Tài liệu | S23 | Later |
| TPL-07 | Lồng template tối đa ba tầng. | Chỉ áp dụng cho database template. | Tài liệu | S23 | Later |

## 13. Lịch sử trang

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| HIS-01 | Notion ghi một phiên bản mỗi 10 phút khi đang sửa, và ghi thêm một phiên bản 2 phút sau lần sửa cuối. | Đây là tần suất snapshot làm căn cứ thiết kế. | Tài liệu | S11 | Later |
| HIS-02 | Xem các phiên bản cũ. Hệ thống làm nổi bật thay đổi văn bản, block thêm hoặc xóa, và một số thay đổi khác. | Không ghi lại mọi thay đổi. Trên mobile không so sánh chi tiết. | Tài liệu | S11 | Later |
| HIS-03 | Khôi phục toàn trang từ phiên bản cũ, hoặc copy vài block từ phiên bản cũ. | Khôi phục không làm mất phiên bản hiện tại. | Tài liệu | S11 | Later |
| HIS-04 | Thời hạn lịch sử: 7 ngày (Free), 30 ngày (Plus), 90 ngày (Business), không giới hạn (Enterprise). | Thời hạn là quyết định thương mại của Notion, không áp dụng cho NE. | Tài liệu | S11, S03 | Later |
| HIS-05 | Khôi phục database không khôi phục nội dung trong từng page con. | Phải khôi phục từng page nếu cần. | Tài liệu | S11 | Later |
| HIS-06 | Snapshot mã hóa theo chu kỳ, cắt log CRDT để giữ dung lượng nhỏ. | Suy luận từ P01 và S37 (CRDT cho trang offline). Không phải tính năng người dùng. | Suy luận | S37 | Must |

## 14. Offline và đồng bộ

Notion dùng offline theo từng trang. NE là local-first nên mọi dữ liệu có sẵn cục bộ. Các mục dưới đây dùng để tham khảo quy tắc xung đột và hiển thị, không phải để sao chép mô hình tải theo trang.

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| OFF-01 | Notion cho phép bật cờ đánh dấu offline cho từng trang trên app desktop hoặc mobile. Web không hỗ trợ. | NE không cần cờ này, vì mọi trang đã có cục bộ (suy luận từ P01). | Tài liệu / Suy luận | S13 | Must |
| OFF-02 | Trang đánh dấu offline được cập nhật nền khi thiết bị khác sửa. | Đây là hành vi đồng bộ cốt lõi. | Tài liệu | S13, S14 | Must |
| OFF-03 | Trang offline chỉ được mở khi đã tải đủ dữ liệu. Notion không hiện trang thiếu nội dung. | Đây là nguyên tắc thiết kế có chủ đích. | Tài liệu | S37 | Must |
| OFF-04 | Trang gần đây và trang yêu thích tự tải offline với gói trả phí. | Hai nguồn mâu thuẫn về gói nào được hưởng (xem public-sources.md, mục G). | Tài liệu / Chưa rõ | S13, S14 | Later |
| OFF-05 | Subpage của trang offline không tự tải theo. | Không áp dụng cho NE. | Tài liệu | S13 | Later |
| OFF-06 | Database offline tải 50 hàng đầu của view đầu tiên. | Giới hạn này đến từ mô hình tải theo trang của Notion. | Tài liệu | S13 | Later |
| OFF-07 | Khi offline, người dùng tạo được page mới và sửa đa số block. Embed, form, button và block AI không dùng được. | NE nên cho sửa mọi block có trong v1 khi offline. | Tài liệu | S13, S14 | Must |
| OFF-08 | Khi offline không chia sẻ hoặc đổi quyền được. | Không có chia sẻ trong v1. | Tài liệu | S13, S14 | Later |
| OFF-09 | Chỉnh sửa offline lưu cục bộ, sau đó tự đồng bộ. Text được gộp tự động. Thuộc tính không phải text, ví dụ select, khi hai người cùng sửa thì chỉ một giá trị được giữ. | Đây là quy tắc xung đột cần quyết định cho NE, xem mục 17. | Tài liệu | S13, S14 | Must |
| OFF-10 | Notion dùng CRDT cho trang offline để giải xung đột. | Phù hợp với lựa chọn Loro trong P01. Notion không nói dùng CRDT cho mọi trang. | Tài liệu | S37 | Must |
| OFF-11 | Khi tìm kiếm offline, các trang đã tải hiện trước, trang chưa tải bị làm mờ. | Hành vi hiển thị, không phải đồng bộ. | Tài liệu | S14 | Should |
| OFF-12 | Có chỉ báo trạng thái đồng bộ. | Hình thức chỉ báo không được mô tả. | Tài liệu | S14 | Should |
| OFF-13 | Mỗi thiết bị lưu thời điểm tải lần cuối của từng trang. Khi nối lại mạng, chỉ tải trang có phiên bản mới hơn. | Notion dùng timestamp. NE dùng version của CRDT, cơ chế khác. | Tài liệu / Suy luận | S37 | Must |
| OFF-14 | Một trang có thể offline vì nhiều lý do. Chỉ bỏ offline khi lý do cuối cùng biến mất. | Cơ chế này thuộc mô hình tải theo trang, NE không cần. | Tài liệu | S37 | Later |
| OFF-15 | Hành vi khi hai thiết bị cùng sửa một block trong lúc offline, và chỉnh sửa cấu trúc như chuyển page. | Notion không mô tả chi tiết. | Chưa rõ | S37, S13 | Must |
| OFF-16 | Giới hạn chính xác của offline, như số trang và dung lượng tối đa. | Notion không công bố. | Chưa rõ | S13 | Must |

## 15. Nhập và xuất

| ID | Hành vi | Ghi chú biên | Loại | Nguồn | Ưu tiên |
|---|---|---|---|---|---|
| IMP-01 | Nhập file Markdown (.md, .markdown) và văn bản thuần (.txt). Có thể chọn nhiều file trong một lần. | Chỉ markdown chuẩn được nhập đúng. Anchor link và cú pháp mở rộng không nhập đúng. | Tài liệu | S15 | Must |
| IMP-02 | Nhập một thư mục qua file ZIP để giữ cấu trúc. | Giới hạn ZIP là 5 GB. Trên 10.000 file dễ lỗi. File ẩn như .DS_Store có thể gây lỗi. | Tài liệu | S15 | Must |
| IMP-03 | Nhập HTML. Heading H4 trở xuống thành H3. Script, embed và style bị bỏ. | Ngoài v1 vì không nằm trong Markdown chuẩn. | Tài liệu | S15 | Later |
| IMP-04 | Nhập CSV thành database. Hàng thành page, cột thành property. | CSV chỉ thêm hàng, không cập nhật hàng cũ. Cần UTF-8. | Tài liệu | S15 | Should |
| IMP-05 | Nhập file Word và PDF. | Ngoài v1. | Tài liệu | S15 | Later |
| IMP-06 | Trang nhập vào mặc định nằm trong khu riêng tư. Có thể chuyển sau khi nhập. | Ở NE, chọn thư mục đích khi nhập. | Tài liệu | S15 | Should |
| IMP-07 | Có danh sách tiến độ nhập, phân loại đang chạy và hoàn tất. | Không có tiến độ chi tiết theo file. | Tài liệu | S15 | Should |
| IMP-08 | Khi nhập lỗi, cần kiểm tra và xóa page đã nhập dở trước khi thử lại. | Notion yêu cầu người dùng tự dọn. | Tài liệu | S15 | Should |
| EXP-01 | Xuất Markdown: mỗi page thành một file .md. Bật tùy chọn xuất kèm subpage để xuất cả cây dưới dạng thư mục. | Đây là đường xuất chính cho v1. | Tài liệu | S16 | Must |
| EXP-02 | Callout không có dạng tương đương trong Markdown, nên Notion xuất thành HTML. | NE cần quyết định cách xuất callout, xem mục 17. | Tài liệu / Suy luận | S16 | Must |
| EXP-03 | Database xuất thành CSV, kèm file .md cho từng page con. Chỉ xuất view hiện tại hoặc view mặc định. | Phụ thuộc database. | Tài liệu | S16 | Later |
| EXP-04 | Xuất PDF và HTML. | Không cần cho v1. | Tài liệu | S16 | Later |
| EXP-05 | Xuất toàn workspace gửi một liên kết tải qua email. Liên kết hết hạn sau 7 ngày. Xuất có thể mất đến 30 giờ. | NE là local-first, sao lưu tương đương là sao chép file dữ liệu. | Tài liệu / Suy luận | S16 | Should |
| EXP-06 | Trên Windows, ZIP có đường dẫn lớn hơn 260 ký tự sẽ không mở được. Notion có tùy chọn không tạo thư mục con để tránh lỗi. | NE trên Windows phải kiểm soát độ dài đường dẫn khi xuất. | Tài liệu / Suy luận | S16 | Must |
| EXP-07 | Nhập lại file đã xuất không khôi phục đúng workspace. | Xuất là một chiều với mất mát. NE cần tài liệu hóa phần mất mát. | Tài liệu / Suy luận | S16 | Must |
| EXP-08 | Xuất comment và lịch sử. | Không có trong v1. | Tài liệu | S16 | Later |
| EXP-09 | Tên file khi xuất và ký tự không hợp lệ trên Windows. | Notion không mô tả. | Chưa rõ | S16 | Must |

## 16. Tổng hợp phạm vi v1 (Must)

Thứ tự dưới đây là thứ tự phụ thuộc khi xây dựng, không phải mức độ quan trọng.

1. Mô hình block dạng cây có parent và danh sách con (CM-01 đến CM-03, CM-10, CM-12).
2. 15 loại block Must (BLK-01 đến BLK-15).
3. Slash menu, markdown shortcut và phím tắt tối thiểu (SLA-01 đến SLA-05, KEY-01 đến KEY-09, KEY-11, KEY-13, KEY-15).
4. Cây trang, kéo-thả trang và block, thùng rác có khôi phục (TRE-01 đến TRE-04, DND-01, DND-04 đến DND-06, DND-09, DND-10, TRA-01 đến TRA-03, TRA-07).
5. Tìm kiếm toàn văn và tìm trong trang (SRC-01, SRC-03, SRC-05, SRC-07, SRC-08, SRC-10 đến SRC-12).
6. Liên kết inline, backlink (LNK-01 đến LNK-04, LNK-08).
7. Sửa offline, đồng bộ và xung đột (OFF-01 đến OFF-03, OFF-07, OFF-09, OFF-10, OFF-13, OFF-15, OFF-16, HIS-06).
8. Nhập và xuất Markdown (IMP-01, IMP-02, EXP-01, EXP-02, EXP-06, EXP-07, EXP-09).

## 17. Khuyến nghị xếp hạng cho các quyết định mở

Mỗi quyết định dưới đây có một lựa chọn được xếp hạng. Lý do và đánh đổi được ghi cùng.

**Q1. Dialect Markdown cho xuất và nhập.**

- Phương án A (xếp hạng 1): dùng CommonMark và GFM cho các block có tương đương, và xuất callout, toggle, cột thành dạng có chú thích. Ưu điểm: đọc được bằng mọi trình soạn thảo, dễ kiểm chứng. Nhược điểm: mất cấu trúc một phần.
- Phương án B (xếp hạng 2, dùng kết hợp với A): thêm một file JSON kèm theo để khôi phục đầy đủ. Ưu điểm: không mất dữ liệu khi nhập lại trong NE. Nhược điểm: file không đọc được bằng công cụ khác.
- Phương án C (xếp hạng 3, không khuyến nghị): tạo dialect có tag riêng giống hướng của Notion (S35). Nhược điểm: dễ trùng với đặc tả của bên khác, và không có lợi ích rõ cho v1.

Khuyến nghị: A cho v1, cộng B nếu có thời gian. Không dùng C.

**Q2. Nghĩa của ">" và '"' khi nhập markdown.**

- Phương án A (xếp hạng 1): ">" là quote theo markdown chuẩn, và toggle có một cú pháp riêng khi xuất. Ưu điểm: nhập file markdown bên ngoài đúng. Nhược điểm: lệch với Notion.
- Phương án B (xếp hạng 2): giữ hành vi Notion, ">" là toggle. Nhược điểm: file markdown bên ngoài bị nhập sai.

Khuyến nghị: A, vì nhập đúng quan trọng hơn khớp Notion.

**Q3. Block liên kết tới page (BLK-13).**

- Phương án A (xếp hạng 1): liên kết không tạo quan hệ cha con, chỉ hiện như một thẻ liên kết. Ưu điểm: giữ cây có một cha, phù hợp movable tree. Nhược điểm: khác với sidebar của Notion.
- Phương án B (xếp hạng 2): liên kết tạo con trong sidebar như Notion. Nhược điểm: cùng một page có hai cha, xung đột với mô hình cây.

Khuyến nghị: A. Lý do: Notion đã bỏ mô hình nhiều cha vì không xác định được quyền (CM-03).

**Q4. Xóa trang trong thùng rác.**

- Phương án A (xếp hạng 1): giữ trang trong thùng rác cho đến khi người dùng xóa tay. Thêm tự dọn sau 30 ngày ở v1.1. Ưu điểm: đơn giản, ít mất dữ liệu. Nhược điểm: thùng rác có thể đầy.
- Phương án B (xếp hạng 2): tự dọn sau 30 ngày rồi xóa hẳn sau 30 ngày nữa, như Notion. Nhược điểm: cần bộ đếm thời gian và trạng thái trung gian.

Khuyến nghị: A cho v1. Phương án B là Should (TRA-04).

## 18. Câu hỏi chưa giải quyết

1. Hành vi khi kéo page vào chính con của nó (DND-09) chưa được công bố. Cần kiểm chứng bằng tay.
2. Gói nào được tự tải offline: S13 và S14 mâu thuẫn (OFF-04).
3. Xung đột khi hai thiết bị cùng sửa một block hoặc chuyển cùng một page (OFF-15) chưa được mô tả chi tiết.
4. Giới hạn offline về số trang và dung lượng (OFF-16) không được công bố.
5. Tìm không dấu tiếng Việt (SRC-10) và thứ tự sort tiếng Việt (DB-10) chưa được công bố.
6. Hành vi khi xóa trang đang được liên kết (LNK-08) và khi khôi phục trang có cha đã bị xóa (TRA-02) chưa rõ.
7. Cách xử lý fence markdown trong code block (BLK-10), cách đánh số lại danh sách và to-do khi xóa một mục ở giữa (BLK-04, BLK-05) chưa rõ.
8. Phạm vi undo (TRA-07) chưa rõ.
9. Tên file và ký tự không hợp lệ khi xuất trên Windows (EXP-09) chưa rõ.
10. Thời gian phản hồi tìm kiếm không được công bố (SRC-11). Mục tiêu 50 ms là của P01, chưa kiểm chứng.

Các câu hỏi 1, 2, 3, 4, 5, 6, 8 và 9 được đưa vào [hands-on-checklist.md](./hands-on-checklist.md).
