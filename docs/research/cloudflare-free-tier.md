# Cloudflare Free tier cho sync server Note Everywhere

Ngày nghiên cứu: 2026-11-21 (Asia/Saigon). Phạm vi: Workers Free, Durable Objects (DO) Free, WebSocket Hibernation, R2 Free, workers.dev, D1 (so sánh), budget một user ba thiết bị.

Nguồn: chỉ dùng trang chính thức developers.cloudflare.com, lấy bản markdown trực tiếp bằng curl (không dùng tóm tắt của WebFetch). Trang Workers limits ghi `dateModified` 2026-11-19. Nguồn ngoài duy nhất là một thread cộng đồng, được đánh dấu rõ.

Quy ước đánh dấu:
- **[V]** = verified on official page (đã đọc nguyên văn trang chính thức).
- **[NV]** = not verified (không tìm thấy xác nhận chính thức, hoặc chỉ có nguồn không chính thức).

## Tóm tắt

- Free tier đủ cho một user, ba thiết bị, theo budget giả định ở mục 7. Không giới hạn đếm được nào bị vượt. Giới hạn chặt nhất theo tỉ lệ là DO duration, và chỉ trong kịch bản xấu nhất khi DO không hibernate.
- Giả định "script size nén" trong yêu cầu là sai. Trang chính thức ghi Worker size là 64 MiB uncompressed và không có giới hạn nén [V].
- Rủi ro lớn nhất không đo được là giới hạn CPU 10 ms mỗi request trên Free. Cần benchmark workers-rs thực tế trước khi chốt kiến trúc.
- Không cần D1. DO SQLite đã có cùng hạn mức rows read/written như D1 Free.
- R2 Free có hạn mức đủ lớn. Chưa rõ R2 có bắt buộc gắn thẻ thanh toán dù chỉ dùng trong hạn mức Free hay không [NV]. Đây là điểm cần kiểm tra trên tài khoản thật.
- workers.dev chặn client gửi User-Agent mặc định (error 1010). Client sync phải đặt User-Agent mô tả rõ ứng dụng [V].

## 1. Workers Free

Nguồn: https://developers.cloudflare.com/workers/platform/limits/ và https://developers.cloudflare.com/workers/platform/pricing/

| Hạng mục | Giới hạn Free | Trạng thái |
|---|---|---|
| Requests | 100.000 mỗi ngày, reset 00:00 UTC (07:00 giờ Asia/Saigon). Vượt thì trả Error 1027, route fail open hoặc fail closed tùy cấu hình | [V] |
| CPU time mỗi HTTP request | 10 ms. Vượt thì Error 1102 "Worker exceeded resource limits". Trang ghi Worker trung bình dùng khoảng 2,2 ms, xử lý nặng 10–20 ms | [V] |
| Wall time HTTP request | Không giới hạn khi client còn kết nối | [V] |
| Memory | 128 MB mỗi isolate, tính cả vùng nhớ WebAssembly | [V] |
| Subrequests | 50 mỗi request (fetch, R2, KV, D1 đều tính). Subrequest tới internal service là 1.000 | [V] |
| Kết nối outbound đồng thời | 6 | [V] |
| Worker size | 64 MiB uncompressed. Không có giới hạn compressed | [V] |
| Startup time (global scope) | 1 giây, lỗi 10021 nếu vượt | [V] |
| Số Worker mỗi account | 100 | [V] |
| Env var | 64 mỗi Worker, 5 KB mỗi biến | [V] |
| Script metadata (bindings) | 1 MB mỗi script | [V] (trang D1 limits, footnote 2) |
| Request body khi upload qua domain Free | 100 MB | [V] |

**Rust/WASM (workers-rs):** trang limits không có giới hạn riêng cho Rust. Các giới hạn chung áp dụng, gồm memory 128 MB tính cả WebAssembly và startup 1 giây. Trang hướng dẫn Rust khuyên bật `lto`, `strip`, `codegen-units = 1` và để `worker-build` chạy `wasm-opt` để giảm size và startup [V] (https://developers.cloudflare.com/workers/languages/rust/).

**Kích thước workers-rs điển hình:** [NV]. Không tìm thấy số liệu chính thức. Ước lượng của tôi là vài trăm KB đến vài MB sau khi compress, chỉ đủ để kết luận nằm xa giới hạn 64 MiB. Cần đo bằng `wrangler deploy --dry-run` (trang limits ghi cách đo, ví dụ kết quả `Total Upload` là uncompressed, `gzip` chỉ để tham khảo).

## 2. Durable Objects Free

Nguồn: https://developers.cloudflare.com/durable-objects/platform/pricing/ và https://developers.cloudflare.com/durable-objects/platform/limits/

- **Có sẵn trên Free:** có [V]. Chỉ được dùng SQLite-backed DO. Key-value backend chỉ dành cho Paid [V].
- **Hạn mức ngày Free** (reset 00:00 UTC) [V]:

| Hạng mục | Free |
|---|---|
| Requests | 100.000 mỗi ngày. Gồm HTTP request, RPC session, alarm. Mỗi lần gọi RPC method là một request |
| Duration | 13.000 GB-s mỗi ngày |
| Rows read | 5 triệu mỗi ngày |
| Rows written | 100.000 mỗi ngày (gồm cả delete và setAlarm) |
| SQL stored data | 5 GB tổng cho toàn account |

- **Storage mỗi object: mâu thuẫn trong trang chính thức.** Bảng limits ghi 10 GB mỗi DO, không nêu plan. Phần FAQ trên cùng trang ghi 10 GB cho Paid và 1 GB cho Free [V cả hai, chưa giải quyết được]. Budget dùng 50 MB nên không ảnh hưởng kết luận.
- **Khi vượt hạn mức Free:** "further operations of that type will fail with an error", tức các thao tác cùng loại sẽ lỗi, không bị tính tiền [V]. Mã lỗi cụ thể và thời điểm phục hồi ngoài reset 00:00 UTC chưa được ghi [NV].
- **Khi storage đầy:** ghi lỗi `SQLITE_FULL`, đọc và xóa vẫn chạy [V].
- **Duration:** tính theo wall-clock khi DO đang active hoặc idle nhưng không đủ điều kiện hibernate. Mỗi DO bị tính 128 MB bộ nhớ cố định, không phụ thuộc mức dùng thực [V].
- **Giới hạn khác:** Free được tối đa 100 DO class mỗi account [V]. Soft limit 1.000 request/giây mỗi object [V].

## 3. WebSocket Hibernation

Nguồn: https://developers.cloudflare.com/durable-objects/best-practices/websockets/, https://developers.cloudflare.com/durable-objects/concepts/durable-object-lifecycle/, và trang pricing ở mục 2.

- **Cách đếm request:** Upgrade WebSocket là một request [V]. Trang Workers pricing ghi các message đi qua Worker không tính là request [V]. Tại DO, mỗi message tới DO được tính theo cơ chế riêng (xem dưới).
- **Tỉ lệ 20:1:** trang pricing ghi "For compute requests billing-only, a 20:1 ratio is applied to incoming WebSocket messages". Ví dụ: 100 message được tính là 5 request [V]. Trang không nói rõ tỉ lệ này có áp dụng cho bộ đếm 100.000 request/ngày của Free hay không [NV]. Budget ở mục 7 đếm 1:1 để làm trường hợp xấu.
- **Ping và auto-response:** ping protocol được runtime tự trả lời, không đánh thức DO [V]. `setWebSocketAutoResponse()` không tốn thời gian duration [V].
- **Hibernation trên Free:** trang không ghi giới hạn theo plan. Free chỉ bị giới hạn ở backend lưu trữ (SQLite). Không có văn bản chính thức nào xác nhận tường minh cho Free [NV].
- **Điều kiện hibernate** [V]: không còn request hay event đang xử lý, không có `setTimeout` hoặc `setInterval`, và DO idle khoảng 10 giây. Alarm và request tới nơi cũng ngăn hibernate [V].
- **Hành vi khi hibernate:** WebSocket vẫn kết nối với client. Khi có message, constructor chạy lại. Trạng thái theo connection phải lưu bằng `serializeAttachment` [V].
- **Outbound WebSocket** (DO tự mở kết nối ra ngoài) không hibernate, và giữ DO trong bộ nhớ tối đa 15 phút [V].

## 4. R2 Free

Nguồn: https://developers.cloudflare.com/r2/pricing/, https://developers.cloudflare.com/r2/platform/limits/, https://developers.cloudflare.com/r2/get-started/

| Hạng mục | Free | Trạng thái |
|---|---|---|
| Storage (Standard) | 10 GB-month mỗi tháng | [V] |
| Storage (Infrequent Access) | Không có free tier | [V] |
| Class A operations | 1 triệu request mỗi tháng (PutObject, ListObjects, CreateMultipartUpload, UploadPart...) | [V] |
| Class B operations | 10 triệu request mỗi tháng (GetObject, HeadObject...) | [V] |
| Delete | Miễn phí (DeleteObject, AbortMultipartUpload) | [V] |
| Egress ra internet | Miễn phí | [V] |
| Object tối đa | 5 GiB mỗi single upload, 4,995 TiB multipart, tối đa 10.000 part | [V] |
| Ghi đồng thời cùng một key | 1 lần mỗi giây, vượt thì HTTP 429 | [V] |

- **Yêu cầu thẻ thanh toán:** trang get-started ghi "Complete the checkout flow to add an R2 subscription to your account" và "R2 is free to get started with included free monthly usage" [V]. Trang không nói rõ có bắt buộc gắn thẻ khi chỉ dùng trong hạn mức Free hay không [NV]. Một thread cộng đồng (không chính thức) báo cáo rằng checkout yêu cầu thẻ: https://community.cloudflare.com/t/why-using-r2-free-tier-involves-giving-card-info/945179. Kết luận: cần thử checkout trên account thật trước khi chốt. Nếu thẻ là bắt buộc, đây là một điều kiện đáng kể dù chi phí thực tế bằng 0.
- **Giới hạn upload qua Worker:** request body qua domain Free tối đa 100 MB. Upload file lớn nên dùng presigned URL hoặc multipart để không đi qua Worker [V].

## 5. workers.dev

Nguồn: https://developers.cloudflare.com/workers/configuration/routing/workers-dev/

- **Mục đích:** Free website, dành cho dự án cá nhân, không business-critical. Trang khuyên production nên dùng route hoặc custom domain [V]. Custom domain không bắt buộc để deploy [V].
- **Rate limit riêng cho workers.dev:** trang không ghi rate limit nào riêng. Chỉ áp dụng giới hạn của plan (100.000 request/ngày) [V cho việc không có rate limit riêng được ghi; ngoài ra NV].
- **Browser Integrity Check (BIC):** luôn bật trên zone workers.dev và không tắt được [V]. Client gửi User-Agent mặc định của thư viện (ví dụ Python urllib) bị trả Error 1010 trước khi Worker chạy, và các request đó không hiện trong log Worker [V]. Client sync cần đặt User-Agent mô tả ứng dụng.
- **WebSocket:** được hỗ trợ, upgrade là một request [V].
- **Bảo vệ truy cập:** có thể bật Cloudflare Access cho URL production, preview và version [V]. Có thể tắt workers.dev bằng `workers_dev = false` [V].

## 6. D1 (so sánh, không khuyến nghị)

Nguồn: https://developers.cloudflare.com/d1/platform/pricing/ và https://developers.cloudflare.com/d1/platform/limits/

- Free: 5 triệu rows read mỗi ngày, 100.000 rows written mỗi ngày, 5 GB tổng storage, tối đa 10 database, 500 MB mỗi database, time travel 7 ngày, 50 query mỗi invocation [V].
- Hạn mức rows read/written của D1 giống hệt DO SQLite Free. Trang DO ghi rõ điều này [V].
- Kết luận: không cần D1. Dùng DO SQLite duy nhất cho mỗi user là đủ, tránh thêm một nơi lưu trữ phải đồng bộ.

## 7. Budget cho một user, ba thiết bị

Giả định (đánh dấu là giả định, không phải số liệu đã xác minh):
- 300 lần sửa note mỗi thiết bị mỗi ngày, tổng 900 lần. Trường hợp xấu: mỗi lần sửa là một push riêng, tức 900 push mỗi ngày.
- Mỗi push là 1 request Worker và 1 RPC tới DO. Mỗi push ghi khoảng 3 row vào DO.
- Mỗi thiết bị mở WebSocket 8 giờ mỗi ngày, reconnect khoảng 1 lần mỗi giờ, tức 72 upgrade mỗi ngày cho ba thiết bị.
- 900 message tới DO mỗi ngày (đếm 1:1, không áp dụng 20:1).
- Ping ứng dụng mỗi 30 giây, 8 giờ mỗi thiết bị, tức 2.880 ping mỗi ngày. Trường hợp xấu là không dùng auto-response.
- Đọc khoảng 120.000 rows mỗi ngày cho việc pull và fan-out.
- 2 GB attachment là tổng dung lượng lưu trữ, trung bình 1 MB mỗi file, tức 2.000 file. Mỗi file được upload một lần và tải xuống bởi ba thiết bị, tức 6.000 lượt đọc.
- 50 MB note data nằm trong SQLite của DO.
- Trường hợp xấu về duration: DO không hibernate trong 24 giờ, tức 0,128 GB × 86.400 s = 11.059 GB-s mỗi ngày.

| Hạng mục | Free | Dự kiến | % dùng | Ghi chú |
|---|---|---|---|---|
| Worker requests/ngày | 100.000 | ~972 (push + upgrade). ~8.972 nếu attachment đi qua Worker | 1% – 9% | Message WebSocket qua Worker không tính |
| DO requests/ngày | 100.000 | ~4.750 (có ping, không auto-response). ~1.870 (có auto-response) | 2% – 5% | Mỗi RPC call là một request |
| DO duration/ngày | 13.000 GB-s | ~0 nếu hibernate hoạt động. 11.059 nếu không hibernate | 0% – 85% | Rủi ro lớn nhất nếu hibernate không hoạt động |
| DO rows read/ngày | 5.000.000 | ~120.000 | ~2% | |
| DO rows written/ngày | 100.000 | ~2.700 | ~3% | |
| DO stored data | 5 GB tổng | 50 MB | 1% | |
| R2 storage | 10 GB-month | 2 GB | 20% | Tăng tuyến tính; hết ở khoảng 10 GB |
| R2 Class A/tháng | 1.000.000 | ~2.000 | 0,2% | Tính cho việc upload ban đầu |
| R2 Class B/tháng | 10.000.000 | ~6.000 | 0,06% | Tính cho việc tải về |
| R2 egress | Miễn phí | 0 | 0% | |
| Worker CPU mỗi request | 10 ms | Chưa đo | Không ước lượng được | Rủi ro thật nhất với Rust/WASM |

**Kết luận về giới hạn bị chạm trước:**
1. Không có giới hạn đếm được nào bị vượt trong budget này.
2. Rủi ro lớn nhất là Worker CPU 10 ms mỗi request. Chưa đo được, và Rust/WASM có thể tốn CPU nhiều hơn mức này cho việc khởi tạo và parse. Cần benchmark bằng `wrangler dev --remote` và CPU profile trước khi chốt.
3. Giới hạn chặt nhất theo tỉ lệ là DO duration, 85% trong kịch bản xấu nhất. Trong thực tế, nếu hibernate hoạt động, con số này gần bằng 0.
4. R2 storage là 20%, tăng theo số attachment, không phải giới hạn theo ngày.

**Các yếu tố làm tăng nhanh mức dùng:**
- Mỗi RPC method call tới DO là một request. Thiết kế nhiều lời gọi RPC cho mỗi push sẽ nhân số request lên tương ứng.
- Ping không dùng auto-response sẽ đánh thức DO và tốn duration.
- Attachment đi qua Worker thay vì presigned URL sẽ nhân số Worker request lên khoảng 9 lần.
- Alarm và `setInterval` ngăn hibernate. Không nên dùng cho heartbeat.

## 8. Những điều chưa xác minh được

1. R2 có bắt buộc gắn thẻ khi chỉ dùng trong hạn mức Free không. Cần thử checkout trên account thật [NV].
2. Mã lỗi cụ thể và hành vi phục hồi khi DO vượt hạn mức Free ngoài reset 00:00 UTC [NV].
3. Storage tối đa mỗi DO trên Free: bảng ghi 10 GB, FAQ ghi 1 GB [mâu thuẫn trong trang chính thức, chưa giải quyết].
4. Tỉ lệ 20:1 cho WebSocket message có áp dụng cho bộ đếm 100.000 request/ngày của Free hay không [NV].
5. Hibernation có được xác nhận tường minh cho Free không. Trang chỉ không ghi hạn chế nào [NV].
6. Kích thước thực tế của bundle workers-rs (compressed và uncompressed) [NV].
7. CPU time thực tế của một push trong workers-rs. Cần benchmark [NV].
8. Mâu thuẫn trong trang DO: FAQ nói DO có "same per invocation CPU limits as any Workers", nhưng đồng thời đặt mặc định 30 giây. Cần xác định giới hạn CPU áp dụng cho front Worker và cho DO [mâu thuẫn trong trang chính thức].
9. Không có rate limit riêng cho workers.dev trong trang chính thức. Chưa kiểm tra hành vi thực tế dưới tải [NV].
10. Diễn giải "300 note edits batched into pushes" là 300 push mỗi thiết bị (trường hợp xấu). Nếu batching thực tế gom 10 lần sửa vào một push, số request giảm đáng kể.
11. Các con số trong budget là giả định, không phải đo đạc.

Nguồn chính thức đã đọc:
- https://developers.cloudflare.com/workers/platform/pricing/
- https://developers.cloudflare.com/workers/platform/limits/
- https://developers.cloudflare.com/workers/languages/rust/
- https://developers.cloudflare.com/durable-objects/platform/pricing/
- https://developers.cloudflare.com/durable-objects/platform/limits/
- https://developers.cloudflare.com/durable-objects/best-practices/websockets/
- https://developers.cloudflare.com/durable-objects/concepts/durable-object-lifecycle/
- https://developers.cloudflare.com/r2/pricing/
- https://developers.cloudflare.com/r2/platform/limits/
- https://developers.cloudflare.com/r2/get-started/
- https://developers.cloudflare.com/workers/configuration/routing/workers-dev/
- https://developers.cloudflare.com/d1/platform/pricing/
- https://developers.cloudflare.com/d1/platform/limits/
