# Nguồn công khai đã dùng (Phase 1, Notion)

Ngày đọc tất cả nguồn: 2026-11-21. Mọi nguồn đều là nguồn chính thức của Notion. Không dùng nguồn bên thứ ba cho bất kỳ khẳng định nào. Nội dung được đọc qua bản HTML tải trực tiếp (curl) rồi tách thành văn bản; không dùng tóm tắt của công cụ tìm kiếm làm căn cứ.

Mức tin cậy:
- Trợ giúp: tài liệu người dùng chính thức, cập nhật thường xuyên, nhưng không ghi rõ phiên bản app và đôi khi mô tả tính năng AI/2026 nằm ngoài phạm vi.
- API: đặc tả chính thức, có kiểu dữ liệu và enum rõ ràng; là nguồn đáng tin nhất cho mô hình dữ liệu.
- Blog kỹ thuật: của Notion, nhưng mô tả ở mức kiến trúc, không phải đặc tả. Bài 2021 có thể đã lỗi thời.

## A. Trung tâm trợ giúp (notion.com/help)

| ID | Tiêu đề (mô tả) | URL | Nhà xuất bản | Lastmod trong sitemap | Dùng cho |
|---|---|---|---|---|---|
| S01 | Phím tắt (keyboard shortcuts) | https://www.notion.com/help/keyboard-shortcuts | Notion | 2026-11-20 | Phím tắt, markdown shortcut, slash menu, @ và [[ mention, kéo-thả block, lưu ý xung đột phím trên Windows |
| S02 | Block là gì (what is a block) | https://www.notion.com/help/what-is-a-block | Notion | 2026-11-20 | Khái niệm block, đổi kiểu block, sắp xếp bằng kéo-thả |
| S03 | Block basics (guide) | https://www.notion.com/help/guides/block-basics-build-the-foundation-for-your-teams-pages | Notion | 2026-08-13 | Nhóm block, tay cầm ⋮⋮ để di chuyển, đổi kiểu, đổi màu |
| S04 | Các loại content block (guide) | https://www.notion.com/help/guides/types-of-content-blocks | Notion | 2026-04-29 | Danh sách nhóm block; block có thể kéo-thả, xóa, nhân bản, đổi kiểu, chuyển sang page |
| S05 | Chuyển đổi block (guide) | https://www.notion.com/help/guides/transforming-content-blocks-in-notion | Notion | 2024-03-07 | Đổi block sang block khác và chuyển block thành một page riêng |
| S06 | Điều hướng bằng sidebar | https://www.notion.com/help/navigate-with-the-sidebar | Notion | 2026-11-20 | Cây trang lồng không giới hạn, kéo để lồng/tách trang, khu yêu thích, khu riêng tư, nút xem thêm, thùng rác, thứ tự sắp xếp, cách ẩn/hiện |
| S07 | Tùy chỉnh sidebar | https://www.notion.com/help/customize-your-sidebar | Notion | 2026-10-15 | Shortcut trong sidebar, ghim view database, phạm vi sidebar riêng từng người |
| S08 | Tạo subpage | https://www.notion.com/help/create-a-subpage | Notion | 2026-11-20 | Ba cách tạo subpage, breadcrumb, kéo trang vào/ra khỏi trang cha |
| S09 | Liên kết và backlink | https://www.notion.com/help/create-links-and-backlinks | Notion | 2026-11-20 | Mention trang bằng @, [[ và +; block Link to page; anchor link tới block; backlink và quyền truy cập |
| S10 | Tìm kiếm (search) | https://www.notion.com/help/search | Notion | 2026-11-20 | Mở tìm kiếm, tìm trong trang, bộ lọc và sắp xếp kết quả, tìm trong database, giới hạn tìm kiếm |
| S11 | Xóa và khôi phục nội dung | https://www.notion.com/help/duplicate-delete-and-restore-content | Notion | 2026-11-20 | Xóa block, vòng đời thùng rác (trong thùng rác, đã xóa, xóa vĩnh viễn), lịch sử phiên bản (tần suất lưu, thời hạn, khôi phục) |
| S12 | Lưu trữ trang (archive) | https://www.notion.com/help/archive-pages | Notion | 2026-11-20 | Lưu trữ khác với thùng rác; ẩn khỏi tìm kiếm mặc định |
| S13 | Dùng trang khi offline | https://www.notion.com/help/use-pages-offline | Notion | 2026-11-20 | Đánh dấu offline theo từng trang, tự tải theo gói, giới hạn database, xung đột khi offline |
| S14 | Làm việc offline (guide) | https://www.notion.com/help/guides/working-offline-in-notion-everything-you-need-to-know | Notion | 2026-11-18 | Hành vi offline khi sửa, khi nối lại mạng, giới hạn block; mâu thuẫn với S13 về gói tự tải |
| S15 | Nhập dữ liệu (import) | https://www.notion.com/help/import-data-into-notion | Notion | 2026-11-20 | Định dạng nhập, Markdown và ZIP, giới hạn kích thước, cách ánh xạ, hành vi khi nhập |
| S16 | Xuất nội dung (export) | https://www.notion.com/help/export-your-content | Notion | 2026-11-20 | Xuất PDF, HTML, Markdown và CSV; cấu trúc subpage; callout khi xuất Markdown; lỗi đường dẫn Windows |
| S17 | Database là gì | https://www.notion.com/help/what-is-a-database | Notion | 2026-11-20 | Database là tập hợp page, database cũng là một page, property, nhiều kiểu view |
| S18 | Data source và linked database | https://www.notion.com/help/data-sources-and-linked-databases | Notion | 2026-11-20 | Database gồm một hoặc nhiều data source; linked data source; quyền truy cập |
| S19 | Thuộc tính database (properties) | https://www.notion.com/help/database-properties | Notion | 2026-11-20 | Danh sách kiểu property, giới hạn 500 property, conditional color, comment trên property |
| S20 | View, filter, sort, group | https://www.notion.com/help/views-filters-and-sorts | Notion | 2026-11-20 | Kiểu view, filter lồng tối đa 3 tầng, sort nhiều cấp, group, mở page trong database theo ba cách |
| S21 | Tạo database | https://www.notion.com/help/create-a-database | Notion | 2026-11-20 | Tạo database từ bảng hoặc page, mỗi row là một page |
| S22 | Relation và rollup | https://www.notion.com/help/relations-and-rollups | Notion | 2026-11-20 | Relation một chiều mặc định, relation hai chiều, giới hạn số lượng liên kết |
| S23 | Database templates | https://www.notion.com/help/database-templates | Notion | 2026-11-20 | Template gắn với từng database, lặp lại theo lịch, giới hạn lồng template |
| S24 | Bắt đầu với template | https://www.notion.com/help/start-with-a-template | Notion | 2026-11-20 | Template gallery, cách nhân bản page làm template |
| S25 | Synced block | https://www.notion.com/help/synced-blocks | Notion | 2026-11-20 | Block đồng bộ nhiều bản, unsync, rủi ro khi xóa bản gốc có nhiều bản sao |
| S26 | Cột, heading và divider | https://www.notion.com/help/columns-headings-and-dividers | Notion | 2026-11-20 | Tạo cột bằng kéo-thả, heading toggle, mục lục, simple table, gộp ô |
| S27 | Table view databases | https://www.notion.com/help/tables | Notion | 2026-11-20 | Bảng database, sửa hàng loạt, calculation ở chân cột |
| S28 | Code block | https://www.notion.com/help/code-blocks | Notion | 2026-11-20 | Chọn ngôn ngữ, tô màu cú pháp, wrap, copy, caption |
| S29 | Public page và web publishing | https://www.notion.com/help/public-pages-and-web-publishing | Notion | 2026-11-20 | Chia sẻ công khai là tính năng riêng, dùng để xác nhận phạm vi "Later" |

## B. Tài liệu API công khai (developers.notion.com)

| ID | Tiêu đề (mô tả) | URL | Nhà xuất bản | Lastmod trong sitemap | Dùng cho |
|---|---|---|---|---|---|
| S30 | Block object (reference) | https://developers.notion.com/reference/block | Notion | 2026-11-17 | Cấu trúc block (id, parent, type, has_children, in_trash, payload theo type); danh sách kiểu block; block nào có con; kiểu "unsupported" |
| S31 | Page object (reference) | https://developers.notion.com/reference/page | Notion | 2026-11-12 | Parent của page (page, workspace, data source); page trong database phải tuân theo schema; in_trash |
| S32 | Data source object (reference) | https://developers.notion.com/reference/data-source | Notion | 2026-11-12 | Database là container của data source; data source là bảng dữ liệu; page là item của data source |
| S33 | Working with page content (guide) | https://developers.notion.com/guides/data-apis/working-with-page-content | Notion | 2026-11-13 | Nội dung page là danh sách block con; block có type; block con lồng nhau; phân trang tối đa 100 |
| S34 | Upgrade guide 2025-09-03 | https://developers.notion.com/guides/get-started/upgrade-guide-2025-09-03 | Notion | 2026-11-11 | Database có nhiều data source; thay đổi mô hình database trong API |
| S35 | Notion-flavored Markdown (guide) | https://developers.notion.com/guides/data-apis/notion-flavored-markdown | Notion | 2026-11-19 | Cách Notion biểu diễn các block mà CommonMark không có (toggle, callout, cột, tag con trong trang); dùng để đánh giá lựa chọn Markdown dialect, không sao chép cú pháp |

## C. Blog kỹ thuật của Notion (notion.com/blog)

| ID | Tiêu đề | URL | Nhà xuất bản | Ngày đăng | Dùng cho |
|---|---|---|---|---|---|
| S36 | The data model behind Notion's flexibility (Jake Teton-Landis) | https://www.notion.com/blog/data-model-behind-notion | Notion Engineering | 2021-05-18 | Mô hình block (id, type, properties, content, parent); phân tách type và property; thụt lề là quan hệ cấu trúc; quyền kế thừa theo cây; permission không dùng nhiều parent. Bài cũ (2021), chỉ dùng cho mô hình khái niệm |
| S37 | How we made Notion available offline (Raymond Xu) | https://www.notion.com/blog/how-we-made-notion-available-offline | Notion Engineering | 2025-12-11 | Một trang có thể có nhiều lý do offline; lý do cuối cùng bị xóa mới bỏ offline; đẩy cập nhật qua kênh theo trang; đồng bộ lại theo timestamp; ràng buộc chỉ hiển thị trang offline đầy đủ; CRDT cho trang offline |
| S38 | Herding elephants: sharding Postgres at Notion (Garrett Fidalgo) | https://www.notion.com/blog/sharding-postgres-at-notion | Notion Engineering | 2021-10-06 | Chỉ xác nhận "cây block" là dạng dữ liệu chính, độ sâu và độ rộng thay đổi lớn. Không dùng cho quyết định sản phẩm |
| S39 | Rebuilding Notion's lexical search reindexer | https://www.notion.com/blog/rebuilding-notions-lexical-search-reindexer | Notion Engineering | 2026-08-10 | Mỗi thay đổi block được đưa vào chỉ mục tìm kiếm; quyền truy cập được gắn vào từng tài liệu tìm kiếm; tìm kiếm từ khóa dựa trên Elasticsearch |

## D. Chỉ dùng để tìm URL (không làm căn cứ khẳng định)

| ID | Nguồn | URL | Ghi chú |
|---|---|---|---|
| X01 | Sitemap trợ giúp tiếng Anh | https://www.notion.com/help/sitemap.xml | Dùng để liệt kê slug bài trợ giúp và lastmod. Bỏ qua các bản ngôn ngữ khác |
| X02 | Sitemap developers | https://developers.notion.com/sitemap.xml | Dùng để liệt kê trang API |

## E. Nguồn nội bộ (không phải nguồn công khai, dùng để đối chiếu phạm vi)

| ID | Tài liệu | Đường dẫn |
|---|---|---|
| P01 | Kế hoạch tổng thể | Tài liệu kế hoạch nội bộ, không công bố trong repo |
| P02 | Brief Phase 1 | Tài liệu kế hoạch nội bộ, không công bố trong repo |

## F. Nguồn đã thấy nhưng không dùng

- Các bản dịch của blog offline và blog sharding (cùng nội dung, ngôn ngữ khác): không dùng, vì bản tiếng Anh đã đọc được.
- Bài tóm tắt của bên thứ ba về kiến trúc sync của Notion (Educative): không dùng. Kết quả tìm kiếm cho thấy đây là suy đoán chưa được xác minh.
- Trang "The Great Re-shard" (2023): không liên quan đến sản phẩm.
- Các trang trợ giúp về AI, Meetings, Home, Library, Marketplace, Teamspaces, SCIM và admin API: nằm ngoài phạm vi v1, không đưa vào inventory.
- Trang "Comments, mentions and reminders", "Link previews", "Understanding block usage": đã tải nhưng chưa đọc đầy đủ, không dùng làm căn cứ.

## G. Ghi chú về xung đột giữa các nguồn

- Gói được tự tải offline: S13 nói mọi gói trả phí (kể cả Plus) tự tải trang gần đây và trang yêu thích. S14 nói chỉ Business và Enterprise. Chưa giải quyết được, ghi vào câu hỏi mở.
- Lịch sử phiên bản: S11 nói 7 ngày (Free), 30 ngày (Plus), 90 ngày (Business). S03 chỉ nêu 30 ngày cho Plus. Hai nguồn không mâu thuẫn.
