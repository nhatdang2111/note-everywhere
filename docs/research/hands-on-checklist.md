# Checklist kiểm chứng tay trên Notion (Phase 1)

Mục đích: xác nhận những điểm mà tài liệu công khai không trả lời. Mỗi mục trỏ về ID trong [notion-feature-inventory.md](./notion-feature-inventory.md).

## Điều kiện chạy

- Dùng tài khoản Notion của chính bạn, trong một workspace thử, chỉ có dữ liệu giả. Không dùng dữ liệu khách hàng hay dữ liệu thật.
- Chỉ thao tác như người dùng thường. Không mở công cụ phát triển trình duyệt để xem mạng, không bắt lưu lượng, không đọc file cache hay gói cài đặt của app.
- Ghi bản app và hệ điều hành đã dùng ở đầu kết quả. Desktop và web có thể khác nhau, nên ghi rõ.
- Ghi kết quả vào ghi chú cá nhân ngoài repo. Không đưa ảnh chụp màn hình, chuỗi giao diện hay icon của Notion vào repo. Chỉ đưa mô tả hành vi bằng lời của bạn.
- Mỗi mục có ba phần: thao tác, điều cần quan sát, và kết quả. Nếu kết quả khác tài liệu, ghi rõ tài liệu nào đã nói gì.

## A. Cây trang và kéo-thả

- [ ] **HO-01** (DND-09, CM-05). Tạo ba page lồng nhau A, con B, cháu C. Kéo A vào C, rồi kéo A vào chính B. Quan sát: có bị chặn không, có báo lỗi không, cây có bị vòng hay mất trang không.
- [ ] **HO-02** (DND-10, CM-06). Tạo một toggle có hai block con. Kéo toggle sang vị trí khác trong trang. Quan sát: các block con đi theo không, và thứ tự của chúng có giữ không.
- [ ] **HO-03** (CM-05). Tạo chuỗi page lồng khoảng 30 cấp. Quan sát: có giới hạn độ sâu không, sidebar có chậm hoặc mất hiển thị không.

## B. Block và markdown

- [ ] **HO-04** (SLA-03, BLK-07). Ở đầu dòng gõ ">" rồi Space, sau đó gõ một dòng chữ. Gõ thêm một dòng bắt đầu bằng dấu nháy kép rồi Space. Quan sát: ">" tạo kiểu block gì, dấu nháy kép tạo kiểu gì.
- [ ] **HO-05** (SLA-10). Gõ "# " rồi chữ rồi Enter. Lặp lại với "- ", "[] " và "1. ". Quan sát: block tiếp theo có giữ kiểu hay trở về đoạn văn.
- [ ] **HO-06** (CM-11, KEY-08). Gõ hai đoạn văn liên tiếp. Chọn đoạn thứ hai và nhấn Tab. Quan sát: có thụt lề không, và đoạn thứ hai có thành con của đoạn thứ nhất không.
- [ ] **HO-07** (CM-04). Tạo một to-do đã check. Đổi nó sang heading, rồi callout, rồi quay lại to-do. Quan sát: trạng thái đã check có còn không.

## C. Tìm kiếm và liên kết

- [ ] **HO-08** (SRC-10, DB-10). Tạo trang có tiêu đề "Bản ghi tiếng Việt" và một đoạn có chữ có dấu. Tìm "ban ghi", "bản ghi" và "BẢN GHI". Ghi kết quả từng lần tìm. Nếu có database thử, tạo năm hàng có tên bắt đầu bằng "Ân", "Ánh", "Bình", "Đức", "Cường" rồi sắp xếp tăng dần theo tên. Ghi thứ tự hiện ra.
- [ ] **HO-09** (SRC-07, SRC-08). Đặt chữ "từ khóa thử nghiệm" trong một toggle đang đóng, một callout và một code block. Tìm cụm đó. Sau đó chuyển một trang có chữ đó vào thùng rác và tìm lại. Quan sát: kết quả hiển thị hay không trong từng trường hợp.
- [ ] **HO-10** (LNK-01). Tạo trang B. Tạo trang A có liên kết "@" tới B. Đổi tên B. Quan sát: tên hiển thị trong liên kết ở A có đổi không.
- [ ] **HO-11** (LNK-08). Tạo A liên kết tới B rồi xóa B. Quan sát: liên kết trong A hiện thế nào, và trang B có hiện trong backlink hay không.

## D. Thùng rác và hoàn tác

- [ ] **HO-12** (TRA-02). Tạo cha A có con B. Xóa A, nhờ đó B đi theo. Khôi phục B khi A vẫn còn trong thùng rác. Quan sát: B về đâu. Sau đó thử khôi phục A và quan sát vị trí của B.
- [ ] **HO-13** (TRA-07). Xóa ba block liên tiếp rồi nhấn Undo nhiều lần. Xóa một page rồi thử Undo. Quan sát: mỗi lần Undo khôi phục đúng bao nhiêu bước, và có khôi phục được page không.

## E. Offline và đồng bộ (cần hai thiết bị)

- [ ] **HO-14** (OFF-07). Bật offline cho một trang trên máy A và ngắt mạng. Tạo trang mới và chèn lần lượt các block trong BLK-01 đến BLK-13. Quan sát: block nào không dùng được khi offline.
- [ ] **HO-15** (OFF-09, OFF-15). Trên hai máy cùng mở một trang có sẵn đoạn văn. Ngắt mạng máy A, sửa một đoạn. Trên máy B (có mạng) sửa cùng đoạn. Nối mạng máy A và quan sát kết quả. Lặp lại, lần này máy A sửa thứ tự block (kéo block lên đầu), trong khi máy B thêm một block ở giữa.
- [ ] **HO-16** (OFF-16). Bật offline cho dần 10, 30, 100 trang nhỏ có dữ liệu giả. Quan sát: có thông báo giới hạn không, và tốc độ mở trang có thay đổi không.
- [ ] **HO-17** (OFF-04). Trong Settings, mục Offline (desktop), xem danh sách trang được tải tự động. Chỉ ghi lại, không thay đổi gói. Ghi gói hiện tại của workspace để đối chiếu với S13 và S14.

## F. Nhập và xuất

- [ ] **HO-18** (EXP-02, EXP-01). Tạo trang có toggle, callout, code block, một cột và một subpage. Xuất Markdown với tùy chọn xuất kèm subpage được bật. Mở file bằng trình soạn thảo văn bản thường để xem. Quan sát: callout và toggle xuất ra dạng gì, subpage nằm ở đâu.
- [ ] **HO-19** (EXP-09). Đặt tên trang có dấu hai chấm, dấu gạch chéo và dấu hỏi rồi xuất. Quan sát: tên file bị đổi ra sao.
- [ ] **HO-20** (IMP-01, IMP-02). Tạo một thư mục có hai file .md, một file có anchor link và một file có cú pháp mở rộng. Nén thư mục thành ZIP rồi nhập. Quan sát: cấu trúc thư mục, anchor và cú pháp mở rộng có được giữ không.
- [ ] **HO-21** (EXP-06). Trên Windows, tạo cây sáu cấp có tên dài, xuất ZIP có subpage, rồi mở ZIP. Ghi có lỗi đường dẫn không. Mục này chỉ làm nếu có máy Windows.

## G. Phím tắt

- [ ] **HO-22** (KEY-01, KEY-06). Chọn một đoạn chữ rồi nhấn Cmd/Ctrl+K. Bỏ chọn và nhấn lại. Ghi kết quả từng lần.
- [ ] **HO-23** (KEY-16). Trên Windows, thử các tổ hợp Ctrl+Shift+số và Ctrl+Shift+L. Ghi tổ hợp nào bị hệ điều hành hoặc app khác chiếm trước. Mục này chỉ làm nếu có máy Windows.

## H. Mục chỉ quan sát, không đủ thời gian kiểm trong một lần

- [ ] **HO-24** (HIS-01). Sửa một trang rồi mở lịch sử phiên bản sau 2 phút và sau 10 phút. Ghi số mốc phiên bản xuất hiện. Đối chiếu với S11.
- [ ] **HO-25** (TRA-04). Đưa một trang vào thùng rác và ghi ngày. Kiểm tra lại sau 30 ngày để xác nhận trạng thái "đã xóa". Đây là mục chỉ có thể làm theo thời gian.

## Sau khi làm

- Với mỗi mục, ghi một trong ba kết quả: "xác nhận tài liệu", "khác tài liệu" (kèm mô tả), hoặc "không kiểm được".
- Chuyển các kết quả khác tài liệu vào ghi chú của phase 1. Không sửa inventory trong repo cho đến khi người điều phối duyệt.
- Mục HO-14 đến HO-16 và HO-22 có ảnh hưởng lớn nhất tới quyết định offline và xung đột. Nên làm trước.
