# Hướng dẫn thiết kế giao diện Note Everywhere

- Trạng thái: áp dụng cho UI desktop ở `apps/desktop/src`, ngày 2026-11-21
- Nguồn sự thật của token: `apps/desktop/src/styles/tokens.css`. Tài liệu này giải thích lý do và quy tắc dùng; khi hai nơi lệch nhau thì sửa tài liệu theo file CSS.

## 1. Nhận diện: "giấy và mực"

Note Everywhere trông như một cuốn sổ giấy kem đặt cạnh một ngòi bút mực xanh lục đậm. Nền ấm, chữ gần đen, một màu nhấn duy nhất (teal đậm) cho hành động chính, liên kết và trạng thái đang chọn. Giao diện sáng và tối cùng một họ màu: tối là "sổ dưới đèn bàn", không phải đen xám trung tính.

Nguyên tắc nhận diện:
- **Một màu nhấn.** Teal chỉ xuất hiện ở nút chính, liên kết, focus, mục đang chọn, chỉ báo đồng bộ. Mọi thứ còn lại là sắc nâu trung tính.
- **Phẳng, có viền mảnh.** Phân tách bằng viền 1 px và đổi tông nền, bóng đổ chỉ dùng cho thứ nổi lên trên nội dung (menu, hộp thoại).
- **Chữ là nhân vật chính.** Cột soạn thảo hẹp (46 rem), dòng cao 1,55, tiêu đề trang lớn nhưng không có ảnh bìa hay biểu tượng trang trí.
- **Không mượn nhận diện của sản phẩm khác.** Bố cục ba khung (sidebar, vùng nội dung, bảng phụ) là kiểu chung của app ghi chú. Chúng tôi không dùng lại icon, câu chữ, thứ tự menu hay hình dạng đặc trưng của Notion: tay cầm kéo block là sáu chấm nằm ngoài lề trái của khối, menu chèn khối dùng icon vẽ riêng kèm một dòng mô tả ngắn cho từng khối, trạng thái đồng bộ là một "viên thuốc" ở thanh trên, liên kết trang là thẻ viền nét đứt.

## 2. Màu

Tất cả màu là biến CSS trên `:root` (sáng) và `:root[data-theme="dark"]` (tối). Không viết mã màu cứng trong component; nếu cần màu mới thì thêm token.

| Token | Sáng | Tối | Dùng cho |
|---|---|---|---|
| `--c-bg-app` | `#f4f0e8` | `#1a1814` | nền ngoài cùng, vùng sau hộp thoại |
| `--c-bg-sidebar` | `#ece7dc` | `#141310` | sidebar |
| `--c-bg-surface` | `#fbfaf6` | `#201e19` | vùng soạn thảo, trang |
| `--c-bg-raised` | `#ffffff` | `#2a2721` | menu, hộp thoại, ô nhập |
| `--c-bg-selected` | `#dcd5c5` | `#3a362d` | mục đang chọn |
| `--c-text` | `#25221d` | `#ece7dd` | chữ chính |
| `--c-text-muted` | `#5b5548` | `#b3ac9b` | chữ phụ, nhãn |
| `--c-text-faint` | `#675f51` | `#958e7d` | gợi ý, chữ vô hiệu nhẹ |
| `--c-border` / `--c-border-strong` | `#d5ccbb` / `#7d7561` | `#3b372e` / `#8a8372` | viền trang trí / viền ô nhập |
| `--c-accent` | `#0f6b67` | `#5cc2ba` | nhấn |
| `--c-danger` | `#b3261e` | `#ff8d84` | xoá, lỗi |
| `--c-warning` | `#7a4e00` | `#f0c36a` | cảnh báo |
| `--c-success` | `#1c6f3a` | `#7fd49a` | thành công |

**Tương phản (WCAG AA).** Các tỉ lệ dưới đây tính bằng công thức độ chói tương đối của WCAG 2.x trên đúng các giá trị ở bảng. Chữ thường cần ≥ 4,5:1, thành phần giao diện và viền ô nhập cần ≥ 3:1.

| Cặp | Sáng | Tối |
|---|---|---|
| chữ chính / surface | 15,2 | 13,5 |
| chữ chính / sidebar | 12,9 | 15,1 |
| chữ phụ / surface | 7,1 | 7,4 |
| chữ phụ / sidebar | 6,0 | 8,2 |
| chữ phụ / mục đang chọn | 5,1 | 5,3 |
| chữ gợi ý / surface | 6,0 | 5,1 |
| chữ gợi ý / sidebar | 5,1 | 5,7 |
| nhấn / surface | 6,1 | 7,8 |
| chữ trên nút nhấn / nút nhấn | 6,3 | 8,0 |
| chữ nhấn đậm / nền nhấn nhạt | 7,6 | 8,2 |
| danger / surface | 6,3 | 7,5 |
| danger / nền danger nhạt | 5,0 | 6,0 |
| warning / nền warning nhạt | 5,7 | 7,1 |
| success / nền success nhạt | 5,0 | 6,7 |
| viền ô nhập / surface | 4,4 | 4,4 |
| viền ô nhập / sidebar | 3,7 | 4,9 |

Quy tắc: không dùng màu làm tín hiệu duy nhất. Trạng thái đồng bộ có icon và chữ, lỗi có icon và văn bản, mục đang chọn có nền và `aria-selected`/`aria-current`.

## 3. Chữ

Không đóng gói font: dùng font hệ thống để mở nhanh và khớp macOS và Windows (`--font-ui`: `system-ui`, `-apple-system`, `Segoe UI Variable Text`...). Code dùng `--font-mono`. Cỡ chữ cơ sở của UI 15 px.

| Token | Cỡ | Dùng cho |
|---|---|---|
| `--fs-xs` | 12 px | nhãn phụ, phím tắt |
| `--fs-sm` | 13 px | sidebar, menu, nút |
| `--fs-md` | 15 px | nội dung, ô nhập |
| `--fs-lg` | 17 px | heading 3, tiêu đề hộp thoại |
| `--fs-xl` | 20 px | heading 2, tiêu đề màn hình |
| `--fs-2xl` | 24 px | heading 1 |
| `--fs-3xl` | 30 px | tiêu đề màn hình chào |
| `--fs-title` | 36 px | tiêu đề trang |

Độ đậm: 400 thường, 600 cho tiêu đề và nhãn nhấn mạnh. Không dùng chữ nghiêng cho giao diện (chỉ trong nội dung do người dùng định dạng). Dòng cao `--lh-tight` 1,25 cho tiêu đề, `--lh-body` 1,55 cho đoạn văn.

## 4. Khoảng cách, bo góc, độ nổi

- Lưới 4 px: `--sp-1` 2, `--sp-2` 4, `--sp-3` 8, `--sp-4` 12, `--sp-5` 16, `--sp-6` 24, `--sp-7` 32, `--sp-8` 48 px. Khoảng đệm bên trong thành phần dùng `--sp-3`/`--sp-4`, khoảng giữa các nhóm dùng `--sp-5` trở lên.
- Bo góc: `--r-sm` 4 px (nút, ô nhập, mục cây), `--r-md` 8 px (menu, thẻ), `--r-lg` 12 px (hộp thoại), `--r-pill` (chỉ báo, nhãn tag).
- Độ nổi: `--shadow-popover` cho menu, hộp slash, thanh định dạng; `--shadow-dialog` cho hộp thoại. Không có bóng cho thành phần nằm trong luồng.
- Kích thước chạm tối thiểu 28 px chiều cao cho nút trong thanh công cụ và hàng cây, 32 px cho nút trong biểu mẫu.

## 5. Chuyển động

- Chỉ dùng chuyển động để giải thích thay đổi: hover (`--dur-fast` 120 ms), mở popover và hộp thoại (`--dur-base` 180 ms), thanh tiến trình. Đường cong `--ease`.
- Không có chuyển động trang, không nảy, không xoay vòng vô tận ngoài vòng quay chờ tải và chấm "đang ghi âm".
- `prefers-reduced-motion: reduce`: hai biến thời lượng về 0 ms, vòng quay đổi thành chữ "Loading…" tĩnh, chấm ghi âm không nhấp nháy (chữ "Recording" vẫn hiện).

## 6. Icon

Bộ icon **vẽ riêng** cho dự án, đặt trong `apps/desktop/src/components/common/icons.tsx` và theo giấy phép MIT của dự án; không dùng thư viện icon ngoài và không sao chép icon của sản phẩm khác. Mỗi icon là SVG inline 16×16, nét 1,5 px, `stroke="currentColor"`, đầu nét tròn, không tô màu, nên tự đổi theo chữ và theme. Icon chỉ trang trí thì `aria-hidden`; nút chỉ có icon luôn có `aria-label` và `title`. Block "trang con" và "liên kết trang" dùng hai icon khác nhau (tờ giấy, tờ giấy có mũi tên) để phân biệt. Cơ sở dữ liệu có icon bảng (`database`), lịch sử trang là đồng hồ (`clock`), nhắc việc là cái chuông (`bell`).

## 7. Trạng thái của thành phần

Mọi thành phần tương tác có đủ các trạng thái sau, dựng từ token chứ không viết riêng:

| Trạng thái | Cách thể hiện |
|---|---|
| Mặc định | nền trong suốt hoặc `--c-bg-raised`, chữ `--c-text` |
| Hover | nền `--c-bg-hover` (nút chính: `--c-accent-hover`) |
| Active (đang nhấn) | nền `--c-bg-active` |
| Focus bàn phím | vòng 2 px `--c-focus` cách 1 px, luôn nhìn thấy, không bao giờ `outline: none` nếu không thay bằng vòng tương đương |
| Chọn/hiện tại | nền `--c-bg-selected`, `aria-selected` hoặc `aria-current` |
| Vô hiệu | chữ `--c-text-faint`, `aria-disabled` hoặc `disabled`, không có hover |
| Đang tải | nút giữ nguyên kích thước, mờ đi và đặt `aria-busy`; vùng nội dung dùng vòng quay kèm chữ "Loading…" |
| Lỗi | văn bản `ApiError.message` trong khối `role="alert"` kèm icon cảnh báo, màu `--c-danger` |
| Trống | một câu giải thích và một hành động tiếp theo, không để vùng trắng |

Chỉ báo kéo-thả: đường kẻ 2 px màu nhấn cho "chèn trước/sau", viền nhấn quanh hàng cho "thả vào trong". Thả vào chính node hoặc con cháu của nó thì không hiện chỉ báo và con trỏ ở dạng cấm.

## 8. Bàn phím và truy cập

- Mọi chức năng dùng được bằng bàn phím. Thứ tự Tab theo thứ tự đọc: sidebar, thanh trên, vùng nội dung.
- Phím tắt dùng `Cmd` trên macOS và `Ctrl` trên Windows: `Mod+K` tìm nhanh, `Mod+F` tìm trong trang, `Mod+B/I/E` đậm/nghiêng/code, `Mod+Shift+S` gạch ngang, `Mod+Shift+K` sửa liên kết, `Mod+[` / `Mod+]` lùi/tiến, `Mod+T` tab mới, `Mod+W` đóng tab, `Mod+\` ẩn/hiện sidebar, `Mod+/` mở menu thao tác của block, `Esc` đóng lớp trên cùng hoặc chọn cả block đang đứng, `Shift+↑/↓` mở rộng vùng chọn block.
- Cây trang theo mẫu `tree` của WAI-ARIA: Mũi tên lên/xuống di chuyển, phải mở nhánh, trái đóng nhánh hoặc về cha, Enter mở, F2 đổi tên, Delete chuyển vào thùng rác, phím Menu hoặc Shift+F10 mở menu ngữ cảnh. Kéo-thả luôn có lựa chọn thay bằng menu "Move up/Move down/Move out".
- Hộp thoại dùng `role="dialog"` + `aria-modal`, nhốt focus bên trong, `Esc` đóng, trả focus về phần tử đã mở nó. Menu dùng `role="menu"`, mũi tên điều hướng, `Esc` đóng.
- Hộp tìm nhanh và hộp slash dùng mẫu combobox/listbox: focus ở ô nhập hoặc editor, mục đang chọn công bố qua `aria-activedescendant`.
- Thông báo tạm (toast) nằm trong vùng `aria-live="polite"`, lỗi dùng `role="alert"`.
- Nội dung nhạy cảm (vault) không có trong `aria-live`, không ghi ra console, và biến mất khỏi DOM khi vault khoá.
- Mọi nhãn chữ đi qua module i18n để dịch sang tiếng Việt về sau.

## 9. Chọn framework: Solid

Solid (MIT) biên dịch JSX thành thao tác DOM trực tiếp, không có virtual DOM, lõi khoảng 7 KB nén gzip, hợp với một app có editor ProseMirror tự quản DOM và nhiều cập nhật nhỏ (cây, chỉ báo, popover). Signal là store tự nhiên cho vài store có tên rõ ràng, và `@solidjs/testing-library` cho phép kiểm thử component với Vitest trên jsdom.

## 10. Chữ trong giao diện

Mọi chuỗi hiển thị nằm trong `apps/desktop/src/i18n`: `en.ts` là danh mục gốc (khoá của nó là kiểu `MessageKey`, nên khoá gõ sai là lỗi biên dịch), `vi.ts` là bản tiếng Việt và thiếu khoá nào thì dùng chữ tiếng Anh. Ngôn ngữ chọn trong Cài đặt và lưu ở `localStorage`. Chuỗi có tham số dùng `{tên}`; test kiểm tra bản dịch giữ đúng tham số của bản gốc.

## 11. Phụ thuộc đã thêm và giấy phép

| Gói | Vai trò | Giấy phép |
|---|---|---|
| `solid-js` 1.9 | framework UI (runtime) | MIT |
| `vite-plugin-solid` | biên dịch JSX (dev) | MIT |
| `vitest` | chạy test (dev) | MIT |
| `@solidjs/testing-library` | test component (dev) | MIT |
| `@testing-library/user-event` | mô phỏng thao tác người dùng (dev) | MIT |

Phụ thuộc runtime thêm chỉ có `solid-js`. Các gói ProseMirror, Loro và Tauri đã có từ trước.

## 12. Màu thẻ chọn (select option)

Chín màu của tuỳ chọn trong thuộc tính "chọn một/chọn nhiều" là token cặp nền/chữ `--c-opt-<màu>-bg` và `--c-opt-<màu>-fg` (xám, nâu, cam, vàng, xanh lá, xanh dương, tím, hồng, đỏ), có đủ bản sáng và tối trong `tokens.css`. Tỉ lệ tương phản tính theo WCAG 2.x trên đúng các giá trị đó, tất cả đạt ≥ 7:1:

| Màu | Sáng | Tối |
|---|---|---|
| xám | 8,7 | 8,0 |
| nâu | 7,6 | 8,1 |
| cam | 7,3 | 7,8 |
| vàng | 7,4 | 7,7 |
| xanh lá | 7,3 | 8,3 |
| xanh dương | 8,1 | 8,0 |
| tím | 8,5 | 8,3 |
| hồng | 8,3 | 8,1 |
| đỏ | 7,1 | 7,9 |

Màu không phải tín hiệu duy nhất: thẻ luôn có tên chữ. Component: `OptionChip`.

## 13. Thành phần cho cơ sở dữ liệu, lịch sử, nhắc việc và block đồng bộ

- **Popover** (`components/common/popover.tsx`): bảng neo vào một phần tử cho trình sửa cần nhiều hơn menu (chọn tuỳ chọn, ngày, liên kết hàng, bộ lọc). `role="dialog"`, nhận focus khi mở, `Esc` hoặc bấm ra ngoài hoặc focus rời đi thì đóng và trả focus về phần tử đã mở nó. Không lồng popover trong popover; việc phức tạp hơn dùng hộp thoại.
- **Bảng (table)** dùng `role="grid"`: một ô có `tabindex="0"` tại mỗi thời điểm (roving), mũi tên/Home/End/PageUp/PageDown di chuyển, Enter hoặc F2 sửa, gõ ký tự bắt đầu sửa, Space bật ô đánh dấu, Delete xoá giá trị, `Esc` huỷ, Ctrl/Cmd+Enter mở hàng làm trang, phím Menu hoặc Shift+F10 mở menu hàng. Chỉ các hàng gần khung nhìn nằm trong DOM (chiều cao hàng 36 px, `--db-row-h`); thanh cuộn vẫn có chiều cao thật. Hàng đang tải hiện thanh xám (`cell-skeleton`), không để vùng trắng.
- **Kéo thả** luôn có lựa chọn bằng bàn phím: cột bảng đổi chỗ bằng menu "Move left/right" hoặc nhóm "Properties"; thẻ kanban di chuyển bằng Alt+mũi tên; độ rộng cột đổi bằng mũi tên trên tay nắm. Chỉ báo thả: viền nhấn quanh cột (thả vào cuối) hoặc vạch nhấn 2 px phía trên thẻ (thả trước thẻ).
- **Thẻ, dòng danh sách, ô lịch** là nút hoặc ô lưới có tên truy cập; thẻ có `aria-keyshortcuts`.
- **Hộp xác nhận đổi kiểu thuộc tính** (`ChangeTypeDialog`) luôn nói bằng lời điều gì xảy ra (giữ, đọc thành trống, hay không đổi) và hiện bảng "Hiện tại / Sau khi đổi" cho vài hàng đầu; nút xác nhận không nhận focus đầu tiên.
- **Panel phụ** (`.side-panel`, rộng `--panel-w`) ở mép phải: lịch sử trang và nhắc việc. Là `aside` có nhãn; đóng bằng nút đóng. Lịch sử nói rõ khi lịch sử cũ không còn (khung `.notice`, các phiên bản bị vô hiệu), không giả vờ có bản xem trước.
- **Block đồng bộ** (`.ne-synced`): khung viền nhấn với thanh tiêu đề tô nhạt ghi "Shown on N pages" và nút tuỳ chọn; trạng thái tải và "chưa về máy" là chữ trong `role="status"`. Nội dung bên trong là editor lồng nhau, sửa tại chỗ.
- **Toast** có thể kèm một hành động chữ (ví dụ "Open page" khi nhắc việc đến hạn); hành động là nút thật, toast không tự đóng trong 12 giây với nhắc việc.
- **Chỉ báo cập nhật** (`.update-pill`) là viên thuốc nhấn ở thanh trên, mở Cài đặt; không bao giờ tự cài.
