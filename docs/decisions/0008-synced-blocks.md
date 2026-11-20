# 0008 — Block đồng bộ: tài liệu nội dung dùng chung, trang chỉ giữ một placeholder

- Trạng thái: đã cài đặt phía lõi (`ne-core`, `ne-store`, shell) và kiểm thử; phần giao diện chưa có (xem "Giao diện phải làm gì")

## Bối cảnh
Block đồng bộ là nội dung xuất hiện trên nhiều trang: sửa ở đâu thì mọi nơi hiện cùng nội dung, trên mọi máy. Tài liệu trang theo [0006](./0006-page-document-structure.md) là một cây `LoroMap` do `loro-prosemirror` ghi, block không có id riêng và không di chuyển được như một đơn vị. Vì vậy block đồng bộ không thể là "cùng một block được tham chiếu từ nhiều cây".

## Quyết định
Nội dung dùng chung nằm trong **một tài liệu Loro riêng**, có cấu trúc y hệt một trang (root `doc` gồm các block), cộng root map `ne` với `role = "synced"` để máy nhận lần đầu biết đó là tài liệu gì. Mỗi trang hiện nó giữ một **placeholder**: node `synced_block` với thuộc tính `syncId` là id của tài liệu dùng chung.

| Phần | Nội dung |
|---|---|
| Placeholder trong trang | `nodeName = "synced_block"`, `attributes = { syncId: "<id>" }`, `children` rỗng (atom, như `divider`) |
| Tài liệu dùng chung | Bố cục trang (0006) + root map `ne` `{ role: "synced" }`; id có dạng id node (`0@<peer ngẫu nhiên>`) |
| Sửa nội dung | Là một bản cập nhật Loro bình thường của tài liệu dùng chung; đến mọi máy qua đường đồng bộ có sẵn như một trang |
| Danh sách vị trí xuất hiện | Bảng dẫn xuất `synced_refs (sync_id, page_id)` trong SQLite, tính từ nội dung các trang lúc lập chỉ mục |

Lý do chọn: editor đã biết gắn một ProseMirror vào một `LoroDoc` theo bố cục trang, nên editor lồng bên trong placeholder dùng lại nguyên binding; gộp ký tự, undo theo thiết bị và chuyển trạng thái đều là hành vi của trang, không phải mã mới. Đồng bộ đi qua đường tài liệu có sẵn (`DocRef::Page` với id dạng node), không cần loại tài liệu mới ở phía server hay trong `ne-sync`.

Phương án đã loại: nhúng nội dung vào mỗi trang rồi tự đối chiếu (hai máy sửa hai bản sao sẽ phải gộp tay, và vi phạm "sửa ở đâu cũng cập nhật mọi nơi"); lưu nội dung dạng JSON trong một map chung (mất gộp theo ký tự, một máy ghi đè máy kia).

## Ngữ nghĩa
- **Tạo từ block có sẵn**: chọn một dải block cấp gốc `start..end` của trang. Lõi tạo tài liệu dùng chung chứa các block đó, rồi thay dải đó trong trang bằng một placeholder. Tài liệu dùng chung được lưu và xếp hàng đồng bộ trước, nên không bao giờ có placeholder trỏ vào tài liệu chưa tồn tại. Dải chọn đã chứa placeholder bị từ chối; nội dung dùng chung không chứa block đồng bộ khác (không lồng).
- **Chèn vào trang khác**: thêm một placeholder cùng `syncId` vào vị trí bất kỳ cho phép block tự do (cấp gốc, trong quote, callout, toggle).
- **Danh sách vị trí xuất hiện**: mọi trang có placeholder, kể cả trang trong thùng rác (vì khôi phục được).
- **Bỏ đồng bộ một vị trí**: thay placeholder tại đó bằng bản sao nội dung hiện tại; các vị trí khác vẫn đồng bộ. Nếu đó là vị trí cuối cùng, tài liệu dùng chung bị xoá ngay trong cùng thao tác, vì nội dung giờ chỉ còn trong trang.
- **Khi vị trí cuối cùng biến mất theo cách khác** (xoá placeholder trong editor, xoá hẳn trang): tài liệu dùng chung **được giữ** để undo và khôi phục phiên bản cũ của trang vẫn tìm thấy nó, đồng thời bắt đầu "đồng hồ mồ côi". `synced_gc(tuổi)` xoá những tài liệu đã không có vị trí nào quá `tuổi`; shell gọi với 30 ngày. Chèn lại placeholder dừng đồng hồ.
- **Xung đột**: cùng quy tắc như trang. Hai máy sửa cùng một đoạn: giữ chỉnh sửa của cả hai, gộp theo ký tự.

## Giới hạn đã biết
- Văn bản của block đồng bộ **không** nằm trong tìm kiếm toàn văn của trang chứa nó (placeholder không có chữ); tìm kiếm không thấy nội dung dùng chung.
- Xuất Markdown của một trang bỏ qua placeholder, vì xuất từng trang không có nội dung dùng chung đi kèm.
- ~~Tài liệu dùng chung chưa được nén trên server~~ Đã sửa: `workspace_documents()` trong `session.rs` liệt kê cả `Workspace::synced_documents()`, nên `compact_server_log` (`sync.rs`) nén tài liệu dùng chung như mọi tài liệu khác.
- Máy chưa nhận được tài liệu dùng chung (placeholder đến trước) phải hiện "nội dung chưa có"; `synced_open` trả `not_found` cho tới khi nó đến.
- Chưa có điều khiển trung tâm "chỉ máy này": không có khái niệm quyền; ai sửa cũng cập nhật mọi nơi, đúng yêu cầu.

## Giao diện phải làm gì
Phải ra cùng bản editor biết `synced_block`, theo cảnh báo ở 0006 (editor ném lỗi khi gặp `nodeName` lạ):
1. Thêm node `synced_block` vào schema (`apps/desktop/src/editor/schema.ts`): atom, thuộc tính `syncId` (chuỗi, mặc định rỗng), không có `children`. Node view hiển thị nội dung dùng chung.
2. Node view mở tài liệu qua `synced_open({ id: syncId })` (trả snapshot), dựng một ProseMirror lồng nhau gắn với `LoroDoc` đó bằng cùng `loro-prosemirror`; mỗi cập nhật cục bộ gửi qua `synced_apply_update({ id: syncId, update })`. Nếu lệnh trả `not_found`, hiện trạng thái "nội dung chưa đồng bộ về" và thử lại khi có sự kiện `tree:changed` hoặc `page:remote-update`.
3. Lắng nghe `page:remote-update` với `pageId = syncId` để áp cập nhật từ máy khác vào editor lồng; core đã đánh dấu tài liệu dùng chung là đang mở khi `synced_open` được gọi, và `synced_close` bỏ đánh dấu.
4. Thay đổi cấu trúc trang do core thực hiện (tạo, chèn, bỏ đồng bộ, và cả khôi phục phiên bản) trả về bản cập nhật của trang; UI phải nạp nó vào editor đang mở như một cập nhật từ xa. Core cũng phát sự kiện `page:remote-update` cho các trang đang mở.
5. Menu của placeholder: "Bỏ đồng bộ" (`synced_unsync`), "Xem các trang dùng block này" (`synced_occurrences`), "Chèn block đồng bộ có sẵn" (`synced_list` rồi `synced_insert`). Lệnh "Tạo block đồng bộ" làm việc trên dải block cấp gốc đang chọn (`synced_create`); với block lồng thì UI phải đưa lên cấp gốc hoặc chọn cả khối cha.
6. Không cho dán hoặc chèn placeholder vào trong tài liệu dùng chung (không lồng); phải chặn trước khi gọi lõi.
7. Bật `doc.setRecordTimestamp(true)` và `doc.setChangeMergeInterval(30)` trên mọi `LoroDoc` của editor (trang và tài liệu dùng chung), để lịch sử trang gom được phiên sửa theo thời gian; lõi đặt cùng cấu hình cho bản của nó.
