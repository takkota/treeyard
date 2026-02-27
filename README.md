# worktree-compose (`wtc`)

複数の git worktree で Docker Compose 環境を同時起動する際の**ポート衝突を自動解決**する CLI ツール。

main と各 linked worktree にスロット番号を割り当て、ポート番号を自動オフセットすることでポート衝突なしに並列稼働を実現する。

> **Note:** `worktree-compose` コマンドは短縮形 `wtc` でも実行できる。以降の例では `wtc` を使用する。

## 主な機能

- **ポート自動オフセット** — `docker-compose.yml` のポート定義を解析し、worktree ごとにポートを自動割当
- **共有ネットワーク** — `docker-compose.override.yml` を生成し、worktree 間で Docker ネットワークを共有
- **共有サービス** — DB など 1 つだけ起動すれば十分なサービスを main に集約
- **スロットレジストリ** — ファイルロックによる排他制御で、複数 worktree の同時 init も安全
- **自動初期化フック** — `git worktree add` 時に自動で `wtc init` を実行

## 使い方

### 基本フロー

```bash
# worktree を作成
git worktree add ../feature-branch

# linked worktree で初期化（slot 1、ポートがオフセットされる）
cd ../feature-branch
wtc init

# それぞれで docker compose up -d（ポート衝突なし）
docker compose up -d
```

> linked worktree で `wtc init` を実行すると、main 側の初期化（`docker-compose.override.yml` の生成など）も自動で行われる。main で `wtc init` を明示的に実行する必要はない。

### コマンド一覧

| コマンド | 説明 |
|---|---|
| `wtc init` | ポート割当 + .env 更新 + override.yml 生成 |
| `wtc status` | 全 worktree のスロット・ポート・コンテナ状態を表示 |
| `wtc env` | 計算済み環境変数を stdout に出力（スクリプト連携用） |
| `wtc cleanup` | 現在の worktree をレジストリから削除 |
| `wtc prune` | 存在しない worktree のエントリを掃除 |
| `wtc hooks install` | post-checkout フックを設置（worktree 作成時に自動 init） |
| `wtc hooks uninstall` | フックを削除 |

### オプション

```
-f, --file <FILE>  docker-compose.yml のパスを指定（デフォルト: git toplevel から自動検出）
```

## 仕組み

### 全体像

```
  ~/projects/
  ├── myapp/           (slot 0: main)  ← ベースポート
  ├── myapp-feature-a/ (slot 1: linked worktree)   ← ベースポート + step
  └── myapp-feature-b/ (slot 2: linked worktree)   ← ベースポート + step × 2
```

### ポートオフセットの計算

各 worktree のポートは以下の式で計算される:

```
  割当ポート = base_port + (slot × step)
```

`step` のデフォルトは **10**。`docker-compose.yml` に 3000 と 8080 のポートがある場合:

```
  ┌─────────────────────┬──────┬───────────┬───────────┐
  │  ディレクトリ         │ slot │ WEB_PORT  │ API_PORT  │
  ├─────────────────────┼──────┼───────────┼───────────┤
  │  myapp/              │  0   │   3000    │   8080    │
  │  myapp-feature-a/    │  1   │   3010    │   8090    │
  │  myapp-feature-b/    │  2   │   3020    │   8100    │
  └─────────────────────┴──────┴───────────┴───────────┘
```

### `wtc init` の処理フロー

```
  docker-compose.yml
         │
         ▼
  ┌─────────────────────┐
  │  YAML 解析           │  ${VAR:-DEFAULT}:CONTAINER パターンの
  │  ポート定義を検出     │  ポート定義を自動検出
  └────────┬────────────┘
           │
           ▼
  ┌─────────────────────┐
  │  スロット割当         │  main = slot 0
  │  (排他ロック付き)     │  linked worktree = slot 1, 2, 3...
  └────────┬────────────┘  (.git/worktree-slots に TSV 保存)
           │
           ▼
  ┌─────────────────────┐
  │  ポート計算           │  base_port + (slot × step)
  │  .env 更新           │  ポート変数 + COMPOSE_PROJECT_NAME
  └────────┬────────────┘  + URL 内ポート番号の一括置換
           │
           ▼
  ┌─────────────────────┐
  │  override.yml 生成   │  共有ネットワーク接続
  │                      │  + 共有サービスの profile 制御
  └────────┬────────────┘  + コンテナ名・ボリューム名の分離
           │
           ▼
  ┌─────────────────────┐
  │  Docker ネットワーク  │  {prefix}-shared ネットワークを
  │  作成                │  作成（存在しなければ）
  └─────────────────────┘
```

### 共有サービスの仕組み

DB や Redis など 1 つだけ起動すれば十分なサービスは、main に集約できる。
linked worktree は共有ネットワーク経由で main のサービスに接続する。

```
  myapp/ (slot 0)                 myapp-feature-a/ (slot 1)
  ┌───────────────────┐           ┌───────────────────┐
  │  web   :3000      │           │  web   :3010      │
  │  api   :8080      │           │  api   :8090      │
  │  postgres :5432 ◄─┼───────────┼──(共有ネットワーク)  │
  │  redis    :6379 ◄─┼───────────┼──(共有ネットワーク)  │
  └───────────────────┘           └───────────────────┘
     ▲ 共有サービスは                 ▲ 共有サービスは
       ここでのみ起動                   起動しない
```

## docker-compose.yml の要件

ポートの自動オフセットには、環境変数パターンを使う必要がある:

```yaml
services:
  web:
    ports:
      - "${WEB_PORT:-3000}:3000"   # ✓ 自動検出される
  api:
    ports:
      - "${API_PORT:-8080}:8080"   # ✓ 自動検出される
      - "9229:9229"                 # ✗ ハードコード（警告が出る）

networks:
  app-network:
    name: ${COMPOSE_PROJECT_NAME:-myapp}-network

volumes:
  data:
    name: ${COMPOSE_PROJECT_NAME:-myapp}_data
```

## カスタマイズ

### ポートステップ

`.env` に `WORKTREE_PORT_STEP` を設定すると、スロット間のポート増分を変更できる（デフォルト: 10）。

```
WORKTREE_PORT_STEP=100
```

### 共有サービス

DB など main でのみ起動するサービスを指定できる。main の `.env` に設定する:

```
WORKTREE_SHARED_SERVICES=postgres,redis
```

共有サービスの動作:
- main（slot 0）: 通常通り起動
- linked worktree: Docker Compose profiles により無効化され、共有ネットワーク経由で main のサービスに接続
- 共有サービスのポートはオフセットされない（常にベースポート）

### 自動初期化フック

```bash
wtc hooks install
```

`git worktree add` で新しい worktree を作成した際に、自動で `wtc init` が実行される。

## インストール

### ワンライナー（推奨）

```bash
gh api repos/takkota/worktree-compose/contents/install.sh --jq .content -H 'Accept: application/vnd.github.v3+json' | base64 -d | bash
```

PATH が通った場所（`~/.cargo/bin` または `~/.local/bin`）に自動でインストールされる。

### ソースからビルド

```bash
git clone ssh://git@github.com/takkota/worktree-compose.git
cd worktree-compose
make install
```

インストール先は自動検出される。明示的に指定する場合:

```bash
make install PREFIX=/usr/local
```

### GitHub Releases バイナリ

```bash
gh release download v0.1.0 -R takkota/worktree-compose -p 'worktree-compose-aarch64-darwin' -p 'wtc-aarch64-darwin'
chmod +x worktree-compose-aarch64-darwin wtc-aarch64-darwin
mv worktree-compose-aarch64-darwin ~/.local/bin/worktree-compose
mv wtc-aarch64-darwin ~/.local/bin/wtc
```

## 動作要件

- **Rust 1.80+**（ソースからビルドする場合）
- **Docker Compose V2**（`docker compose` サブコマンド形式）。旧 `docker-compose` CLI（V1）はサポート対象外。

## 内部構造

- **スロットレジストリ**: `.git/worktree-slots` に TSV 形式で保存（全 worktree 共有、`fs2` ファイルロックで排他制御）
- **override.yml**: 全サービスを `{prefix}-shared` 外部ネットワークに接続 + コンテナ名・ボリューム名を `${COMPOSE_PROJECT_NAME}` ベースに上書き
- **派生 URL 自動更新**: `.env` 内の `://localhost:<old_port>` を新ポートに一括置換（単一パス regex で多段置換を回避）
- **YAML 解析**: `serde_yaml_ng` で YAML 1.2 準拠パース。`${VAR:-default}` はプレースホルダーに置換してからパースし、文字列値で復元

## License

MIT
