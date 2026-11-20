# 0007 — Nhận dạng giọng nói offline

- Trạng thái: đạt ngưỡng trên **dữ liệu giả lập bằng text-to-speech**; chưa kiểm chứng bằng giọng người thật và chưa đo trên laptop Windows (ngày 2026-11-21)

## Bối cảnh
Chủ dự án quyết định: nhận dạng giọng nói chỉ chạy offline, không có phương án gửi audio lên cloud. Tính năng chỉ được giữ nếu đạt cả ba ngưỡng sau trên cỡ máy tham chiếu, đo bằng CPU (GPU chỉ để tham khảo):

| Ngưỡng | Yêu cầu |
|---|---|
| Sai số từ (WER) tiếng Việt | dưới 12 % |
| Sai số từ (WER) tiếng Anh | dưới 8 % |
| Độ trễ cho 10 giây lời nói | dưới 5 giây, không dùng GPU rời |

## Cách đo
- Máy: Apple M4 Pro (12 nhân), 24 GB RAM, macOS. Engine: whisper.cpp đi kèm `whisper-rs` 0.16.0 (`whisper-rs-sys` 0.15.0), giải mã greedy (`best_of = 1`), không dùng prompt.
- Model tải từ kho chính thức `ggerganov/whisper.cpp` trên Hugging Face, đã kiểm SHA-256 trùng với giá trị Hugging Face công bố:

| Model | URL | Kích thước | SHA-256 |
|---|---|---|---|
| `ggml-large-v3-turbo-q5_0.bin` | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin` | 574 041 195 byte (547 MiB) | `394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2` |
| `ggml-small-q5_1.bin` | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin` | 190 085 487 byte (181 MiB) | `ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb` |

  Kho chỉ có bản `small` lượng tử hoá `q5_1` (không có `q5_0`), nên dùng bản đó để so sánh.
- Âm thanh: **giả lập bằng macOS `say`** vì chưa có bản thu thật. 20 câu tiếng Việt (giọng Linh, vi_VN, 91,8 giây), 20 câu tiếng Anh (giọng Samantha, en_US, 79,2 giây) và 10 câu trộn Việt-Anh (giọng Linh, 47,6 giây), đổi sang WAV 16 kHz mono bằng `afconvert`. Văn bản các câu nằm ở `spikes/voice/sentences/`; audio và model không được commit.
- WER: Unicode NFC, chữ thường, bỏ dấu câu, **giữ dấu thanh và dấu mũ**; không quy đổi chữ số thành chữ (ví dụ "ba" và "3" tính là hai từ khác nhau).
- Ngôn ngữ ghim theo bộ câu (`vi` cho câu Việt và câu trộn, `en` cho câu Anh). Bộ đo là `crates/ne-voice/examples/measure-transcription.rs`; chạy một lần khởi động trước khi tính giờ.
- Độ trễ có hai cách tính, vì whisper luôn mã hoá một cửa sổ cố định 30 giây nên thời gian xử lý gần như không đổi theo độ dài câu ngắn (câu 4,6 giây tốn gần bằng câu 12 giây):
  1. *RTF tuyến tính*: tổng thời gian chia tổng độ dài audio, nhân 10. Cách này **phóng đại** độ trễ vì các câu thử đều ngắn.
  2. *Câu ~10 giây thực*: nối các câu liền nhau (cách nhau 0,3 giây) thành đoạn dài 12–13 giây rồi đo từng đoạn. Đây là phép đo sát nội dung ngưỡng ("10 giây lời nói"), và 12–13 giây là cận trên của 10 giây.

## Kết quả (model `large-v3-turbo-q5_0`)

| Chế độ | WER Việt | WER Anh | WER trộn | RTF tuyến tính → giây / 10 s (Việt / Anh / trộn) | Đoạn ~12 s: giây trung bình (Việt / Anh / trộn) |
|---|---|---|---|---|---|
| **CPU, 4 luồng (ngưỡng)** | **7,5 %** | **1,8 %** | 32,3 % | 5,32 / 6,07 / 5,09 | **2,54 / 2,48 / 2,51** (tối đa 2,61) |
| CPU, 8 luồng | 7,5 % | 1,8 % | 32,3 % | 4,51 / 5,25 / 4,46 | 2,34 / 2,24 / 2,23 |
| Metal (GPU tích hợp, tham khảo) | 7,5 % | 1,8 % | 32,3 % | 1,74 / 1,96 / 1,63 | 0,87 / 0,84 / 0,85 |
| Metal, tự nhận ngôn ngữ (`auto`) | 7,5 % | 1,8 % | 32,3 % | 3,33 / 3,95 / 3,42 | 1,74 / 1,70 / 1,73 |

Bản CPU được biên dịch không có feature GPU nào. Tự nhận ngôn ngữ cho cùng WER (WER không đổi) nhưng cộng thêm một lượt mã hoá, khoảng 0,85 giây trên Metal.

Model nhỏ để so sánh (`small-q5_1`, CPU, 4 luồng):

| WER Việt | WER Anh | WER trộn | RTF tuyến tính → giây / 10 s | Đoạn ~12 s |
|---|---|---|---|---|
| 14,0 % | 1,8 % | 52,3 % | 1,35 / 1,52 / 1,37 | 0,80 / 0,73 / 0,77 |

## Đối chiếu ngưỡng

| Ngưỡng | Kết quả (turbo q5_0, CPU) | Đạt? |
|---|---|---|
| WER Việt < 12 % | 7,5 % | đạt |
| WER Anh < 8 % | 1,8 % | đạt |
| 10 giây lời nói < 5 giây, CPU | 2,5 giây (đoạn 12–13 giây, 4 luồng); 2,2–2,3 giây với 8 luồng | đạt, dư gần 2 lần |

Model `small-q5_1` **không** đạt ngưỡng tiếng Việt (14,0 %), nên không thể coi là phương án thay thế đủ chuẩn; chỉ nên cung cấp như lựa chọn "máy yếu, chấp nhận kém chính xác".

## Điều cần đọc kỹ trước khi tin các con số

1. **Chỉ đạt trên dữ liệu giả lập.** Giọng TTS sạch, đều, không tạp âm, không ngập ngừng, không vùng miền. Giọng người thật qua micro laptop sẽ cho WER cao hơn, có thể cao hơn nhiều; con số 7,5 % tiếng Việt có ít dư địa so với ngưỡng 12 %. Ngưỡng cần được xác nhận lại bằng bản thu thật trước khi coi tính năng là hoàn tất.
2. **Chưa đo laptop Windows.** Chỉ có Apple M4 Pro. Ngưỡng 5 giây đạt dư gần 2 lần trên M4 Pro, nhưng laptop Windows không GPU rời thường chậm hơn rõ rệt (ít nhân hiệu năng cao, xung nhịp thấp khi dùng pin). Nếu chậm hơn 2 lần thì chạm ngưỡng. Chưa thử Vulkan.
3. **Một phần WER tiếng Việt là khác biệt cách viết, không phải nhận sai.** Model viết số bằng chữ số ("quý 3", "thứ 6", "9h30", "25.000 đồng", "500GB") trong khi câu mẫu viết "quý ba", "9 giờ 30"; ngoài ra có cặp "mã hoá/mã hóa", "khoẻ/khỏe" khác vị trí dấu thanh. Các lỗi này vẫn được tính vào 7,5 % vì phép chuẩn hoá chỉ gồm những bước nêu trên. Cũng có lỗi nhận sai thật (ví dụ "ghi nhớ → duy nhớ", "ghi chú → vi trú", "định kỳ → điện kỳ").
4. **Số WER của câu trộn (32,3 %) không đánh giá được model.** Giọng Linh đọc từ tiếng Anh (deploy, repository, encrypt…) theo âm tiếng Việt nên bản thu không giống người Việt thật sự nói lẫn tiếng Anh. Con số này không thuộc ngưỡng quyết định. Danh sách từ riêng (initial prompt) đã có trong API nhưng chưa được đo.
5. Câu tiếng Anh cho WER 1,8 % chủ yếu do "two/three" thành "2/$3"; chỉ một lỗi nhận sai thật trong 20 câu.
6. Chưa thử PhoWhisper (model tinh chỉnh cho tiếng Việt) vì cần chuyển đổi sang định dạng ggml, nằm ngoài phạm vi lần đo này.
7. Mẫu nhỏ (50 câu, vài trăm từ mỗi ngôn ngữ): chênh lệch vài điểm phần trăm chưa có ý nghĩa thống kê.

## Quyết định
- **Giữ tính năng** theo ngưỡng đã đặt, ghi rõ là đạt trên dữ liệu giả lập. Hoàn tất bước xác nhận bằng giọng người thật và bằng laptop Windows là việc của con người (xem "Chưa kiểm chứng"); nếu bản thu thật vượt 12 % tiếng Việt hoặc laptop Windows vượt 5 giây thì quay lại ADR này để huỷ hoặc đổi model.
- Engine: whisper.cpp qua `whisper-rs`. Model mặc định `large-v3-turbo-q5_0`; `small-q5_1` là lựa chọn phụ. Danh mục model (tên, URL, kích thước, SHA-256) nằm trong `ne-voice::models` và trùng bảng trên.
- Giải mã greedy, không beam search, để giữ độ trễ thấp.
- Cắt khoảng lặng bằng **bộ phát hiện năng lượng ngắn hạn** (khung 20 ms, ngưỡng thích nghi theo mức nền, bỏ tiếng động ngắn dưới 80 ms, giữ đệm 200 ms hai đầu). Đây là cách đơn giản có kiểm thử bằng tín hiệu tổng hợp; nó không phân biệt tiếng nói với tiếng ồn lớn, và không dùng Silero VAD như dự tính ban đầu. Đổi sang Silero chỉ cần thay `vad::speech_range`.
- Tải model bằng `ureq` với TLS của hệ điều hành (`native-tls`: Security.framework trên macOS, SChannel trên Windows), có `Range` để tiếp tục tải dở, kiểm SHA-256 trước khi đổi tên tệp. Không dùng `rustls`/`ring` để tránh yêu cầu NASM trên Windows.
- Lưu ý an toàn trong lớp bọc: `whisper-rs` 0.16 đọc closure huỷ (`set_abort_callback_safe`) sai kiểu nếu truyền closure trần (hành vi không xác định, gây lỗi mã hoá ngẫu nhiên khi đo); code truyền `Box<dyn FnMut() -> bool>` để hai phía khớp kiểu. Kiểm lại khi nâng cấp `whisper-rs`.

## Hệ quả
- Feature `transcribe` (whisper.cpp) **không bật mặc định**. `whisper-rs-sys` cần `cmake` và `libclang` (bindgen) khi biên dịch và Rust 1.88 trở lên; đã kiểm biên dịch trên macOS, **chưa kiểm trên Windows** nên không thể cam kết bộ feature mặc định chứa whisper biên dịch được trên runner Windows tiêu chuẩn. Phần còn lại (thu âm bằng `cpal`, resample, VAD, quản lý model, WER) nằm trong feature mặc định. Ứng dụng desktop bật `transcribe` (và `metal`/`vulkan` theo nền tảng) khi đóng gói.
- Workspace khai báo `rust-version = "1.85"` nhưng `whisper-rs-sys` yêu cầu 1.88; cần nâng `rust-version` khi desktop bật `transcribe`.
- Cả `whisper-rs` và `whisper-rs-sys` dùng giấy phép **Unlicense** (public domain, tương thích MIT). `deny.toml` hiện chưa liệt kê, nên cần thêm `"Unlicense"` vào danh sách `allow` trước khi chạy `cargo deny`.
- `native-tls` kéo `openssl` trên Linux; dự án chỉ nhắm macOS và Windows nên không ảnh hưởng, nhưng build Linux sẽ cần thư viện OpenSSL và ALSA.
- Audio chỉ nằm trong bộ nhớ, không ghi đĩa, không gửi đi đâu. Mạng chỉ dùng để tải tệp model theo yêu cầu người dùng; sau khi tải, tính năng chạy khi tắt mạng hoàn toàn.

## Chưa kiểm chứng
- Giọng người thật (Việt, Anh, trộn) qua micro laptop và ở nơi có tiếng ồn: cần thu lại tập câu `spikes/voice/sentences/` rồi chạy lại `measure-transcription`.
- Laptop Windows không GPU rời (CPU và Vulkan); cũng cần đo máy Mac cũ hơn M4 Pro.
- Biên dịch `ne-voice` với `--features transcribe` trên runner Windows (cần cmake, MSVC, LLVM).
- Thu âm bằng micro thật: bộ thu (`capture`) mới được kiểm bằng test đơn vị trên pipeline chuyển đổi và bằng việc liệt kê thiết bị; chưa khởi động luồng thu vì lần đầu macOS sẽ hỏi quyền micro cho ứng dụng chạy lệnh. Khi ứng dụng desktop đóng gói cần `NSMicrophoneUsageDescription` trong `Info.plist`.
- Các số đo độ trễ có độ dao động giữa các lần chạy (đo một lần mỗi cấu hình).

## Tái tạo số đo
```
sh spikes/voice/synthesize.sh
cargo run --release -p ne-voice --features transcribe --example measure-transcription -- \
  --model spikes/voice/models/ggml-large-v3-turbo-q5_0.bin --threads 4
cargo run --release -p ne-voice --features metal --example measure-transcription -- \
  --model spikes/voice/models/ggml-large-v3-turbo-q5_0.bin --gpu
```
Thêm `--auto` để tự nhận ngôn ngữ và `--verbose` để xem từng câu.
