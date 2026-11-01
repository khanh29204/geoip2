# geoip-rust-service

Dịch vụ tra cứu GeoIP dùng cơ sở dữ liệu GeoLite2 City của MaxMind. API chạy không cần quyền root. Trong container, dịch vụ lắng nghe trên `0.0.0.0:8080`; Compose chỉ publish cổng ra host tại `127.0.0.1:8080`.

## Chạy bằng Docker Compose

```sh
cp GeoIP.conf.example GeoIP.conf
# Điền AccountID và LicenseKey thật trong GeoIP.conf trên máy của bạn; không chia sẻ file này.
mkdir -p geoip-data
```

`geoip-api` khai báo cả `build: .` và `image: ghcr.io/khanh29204/geoip2:latest`: build local sẽ gắn tag GHCR này cho image vừa build. Chọn một trong hai cách chạy dưới đây.

Build và chạy image local:

```sh
docker compose --env-file /dev/null pull geoip-updater
docker compose --env-file /dev/null up -d --build --pull never
```

Chạy image từ GHCR (cần image đã được publish và package đặt public trước):

```sh
docker compose --env-file /dev/null up -d --no-build --pull always
```

`geoip-updater` tải/cập nhật DB mỗi 168 giờ vào `geoip-data/`; API gắn thư mục đó chỉ đọc tại `/data`, với `DB_PATH=/data/GeoLite2-City.mmdb` và `ASN_DB_PATH=/data/GeoLite2-ASN.mmdb`. Compose chỉ đợi updater bắt đầu, không đợi lần tải DB đầu tiên hoàn tất. API chờ cả hai DB ban đầu và kiểm tra độc lập mỗi DB mỗi 5 giây. Cần có cả GeoLite2-City và GeoLite2-ASN trước khi API khởi động; nếu dùng `GeoIP.conf` đã có, tự thêm `GeoLite2-ASN` vào `EditionIDs` (API sẽ tiếp tục chờ nếu thiếu file ASN). Mỗi DB được thay reader độc lập sau khi tải/nạp thành công; lỗi nạp giữ reader cũ của DB đó, DB còn lại vẫn có thể cập nhật. Hai DB không được đảm bảo cùng một phiên bản phát hành.

## Cấu hình và hành vi

- `PORT` chọn cổng API (mặc định `8080`); `DB_PATH` và `ASN_DB_PATH` phải là đường dẫn tới file DB đầy đủ (mặc định lần lượt `/data/GeoLite2-City.mmdb` và `/data/GeoLite2-ASN.mmdb`). Đặt lại các biến này nếu chạy ngoài Compose.
- `GET /health` là health check. API hỗ trợ IPv4/IPv6; trường không có dữ liệu trả về `null`.
- Tra cứu IP: `GET /json/{ip}` và `GET /api/v1/geoip/{ip}` (bí danh). JSON bổ sung `asn` (số nguyên nullable) và `asn_organization` (chuỗi nullable); backend ánh xạ `as` thành `AS<asn>` và `org` thành `asn_organization`, còn `isp` không được suy ra từ ASN. IP sai định dạng trả `400`; trả `200` nếu có bản ghi ở một trong hai DB, `404` nếu cả hai đều không có, và `500` khi lỗi giải mã/hệ thống.
- Cập nhật DB phải dùng file tạm đã tải và xác thực rồi thay bằng atomic rename; không ghi đè file tại chỗ vì API dùng mmap.
- Kết quả GeoLite (ví dụ `1.1.1.1`) có thể thay đổi và fixture kiểm thử không đại diện cho độ chính xác production. RAM dưới 20 MB không được đảm bảo vì mmap có thể đưa các trang DB vào RAM; độ trễ mạng cũng không bằng không.
- API không có xác thực. Docker chỉ publish trên loopback host theo cấu hình mặc định; nếu đổi địa chỉ publish để truy cập từ mạng, hãy đặt sau reverse proxy có xác thực và giới hạn truy cập.

## Tra cứu

Các lệnh dưới đây gọi DB production GeoLite2 City và ASN được Compose cập nhật. Kết quả cho `1.1.1.1` phụ thuộc phiên bản DB:

```sh
curl -i http://127.0.0.1:8080/json/1.1.1.1
curl -i http://127.0.0.1:8080/api/v1/geoip/1.1.1.1
```

IP không hợp lệ trả `400 Bad Request` trên cả hai route:

```sh
curl -i http://127.0.0.1:8080/json/invalid-ip
curl -i http://127.0.0.1:8080/api/v1/geoip/invalid-ip
```

## Kiểm tra local

```sh
cargo fmt --all -- --check
cargo check --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Kiểm tra API/DB dùng fixture MaxMind-DB trong `tests/fixtures/`, không DB production.

Dừng dịch vụ: `docker compose --env-file /dev/null down`.

## Cache build Docker/CI

`cargo-chef` tạo recipe từ manifest, lockfile và cấu trúc target. Khi chỉ sửa mã nguồn, recipe không đổi nên lớp cài dependencies có thể được tái sử dụng; planner và cook phải dùng cùng phiên bản Rust và cùng builder stage. Image cuối là binary chạy không root trên Alpine.

- GitHub Actions (`.github/workflows/docker.yml`) build native qua matrix cho `linux/amd64` và `linux/arm64`; mỗi kiến trúc có scope cache `gha` riêng (`geoip-rust-service-amd64` hoặc `geoip-rust-service-arm64`, `mode=max`). Trên default branch, push hoặc dispatch publish image GHCR với manifest đa kiến trúc kết hợp bằng digest merge, dưới tag `latest` và `sha-<đầy đủ 40 ký tự SHA>`. Pull request và nhánh khác chỉ build, không publish. Runner ARM native cần khả dụng theo plan và giới hạn repository.
- GitLab CI (`.gitlab-ci.yml`) chỉ chạy trên default branch được bảo vệ; Buildx dùng QEMU để build `linux/amd64` và `linux/arm64` rồi publish một manifest đa kiến trúc dưới tag commit SHA bất biến trong `CI_REGISTRY_IMAGE`, cùng một ref `buildcache` riêng. Binfmt được đăng ký bên trong CI Docker-in-Docker. Cache lạnh trên QEMU có thể chậm hơn đáng kể do giả lập. Cần registry khả dụng, Docker runner ở chế độ privileged và volume TLS dùng chung `/certs/client`. Kích hoạt workflow GitLab sau lần push tương lai sẽ publish image và cache, không deploy.
- Cache lạnh sẽ biên dịch dependencies một lần; thay đổi phiên bản dependency làm mất hiệu lực cache là bình thường. Chưa có số đo thời gian hay RAM.

## Dùng image GHCR

Workflow dùng `GITHUB_TOKEN` với quyền `packages: write`, không cần PAT riêng. Nếu package đã tồn tại, có thể cần cấp quyền **Manage Actions access** cho repository trong package settings.

Sau lần push đầu tiên tạo package, owner cần mở [cài đặt package GHCR](https://github.com/users/khanh29204/packages/container/geoip2/settings) và chọn **Change visibility → Public**. Repository công khai không tự làm package công khai; `GITHUB_TOKEN` chuẩn của workflow không thể tự đổi visibility. Khi public, bất kỳ ai cũng có thể tải image này.

```sh
docker pull ghcr.io/khanh29204/geoip2:latest
```

Build local (một kiến trúc):

```sh
docker buildx build --load -t geoip-rust-service:local .
```

Ví dụ build và publish manifest đa kiến trúc theo yêu cầu (không phải bước CI mặc định; cần đăng nhập registry và binfmt đã đăng ký hoặc builder đa kiến trúc native):

```sh
docker buildx build --platform linux/amd64,linux/arm64 --tag REGISTRY/geoip-rust-service:TAG --push .
```
