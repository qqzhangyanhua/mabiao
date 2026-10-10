# 部署远程服务

一个部署对应一个团队（ADR 0026）。

```bash
cp deploy/.env.example deploy/.env      # 改 POSTGRES_PASSWORD
docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d --build
```

## 建第一个管理员

没有注册接口，第一个管理员只能在服务端命令行建：

```bash
docker compose -f deploy/docker-compose.yml --env-file deploy/.env \
  run --rm server create-admin --account admin
```

终端里会提示输入两遍密码（至少 8 个字符）。非交互时设 `MABIAO_ADMIN_PASSWORD`，或从 stdin 传一行。
之后管理员用 `POST /api/v1/admin/accounts` 建成员，`POST /api/v1/admin/accounts/{id}/deactivate` 停用。

## 管理网页

服务镜像自带管理网页（`server/web/`），和 API 同一个端口：浏览器打开服务地址即可，用管理员或成员账号登录。
成员只能看到自己推送的数据，管理员看全员、建 / 停用成员、查看每个成员的最后推送时间与已覆盖到哪天。

网页由 `MABIAO_WEB_DIR` 指向的构建产物目录托管（镜像里已设好）。不设这个变量，服务就只提供 API。
本地开发：在 `server/` 起服务，再在 `server/web/` 里 `pnpm install && pnpm dev`，开发服务器会把 `/api` 代理到 `127.0.0.1:8080`。

## TLS

服务只监听明文 HTTP，且 compose 只把端口绑在 `127.0.0.1:8080`。桌面端强制 https（回环地址除外），
所以对外要在前面放 Caddy / nginx 之类的反向代理终结 TLS。

## 备份

数据都在 `pgdata` 卷里。用 `pg_dump` 备份：

```bash
docker compose -f deploy/docker-compose.yml exec postgres pg_dump -U mabiao mabiao > mabiao.sql
```
