# worktree-compose OSS公開前 総合レビュー

**レビュー日**: 2026-02-26
**対象**: worktree-compose v0.1.0 (全ソースコード 3,660行 / テスト含む)

---

## 総合評価

| カテゴリ | 評価 | 補足 |
|---------|------|------|
| **アーキテクチャ** | ★★★★★ | モジュール分割が明確。責務分離が良い |
| **コア品質 (YAML解析・ポート計算)** | ★★★★☆ | 充実したテストあり。一部エッジケースに課題 |
| **エラーハンドリング** | ★★★☆☆ | anyhow/thiserror活用は良い。一部panic箇所あり |
| **テストカバレッジ** | ★★★☆☆ | core/ は良好だが commands/ は未テスト |
| **CLI UX** | ★★★★☆ | 直感的なコマンド体系。一部改善余地あり |
| **セキュリティ** | ★★★★☆ | コマンドインジェクション対策済。YAML injection要注意 |
| **CI/CD** | ★★★★★ | マルチプラットフォーム、lint厳格 |
| **OSS公開準備** | ★★☆☆☆ | LICENSE ファイル・CHANGELOG 未整備 |

---

## 1. 重要度: 高 — 修正すべき問題

### 1.1 LICENSEファイルが存在しない

Cargo.toml に `license = "MIT"` と宣言されているが、リポジトリルートに `LICENSE` ファイルがない。OSS公開にはライセンスファイルの同梱が必須。

**対応**: MIT LICENSE ファイルをルートに追加する。

### 1.2 ポート番号 > 65535 のサイレント処理

`compose_parser.rs` にて、ポート番号を `parse::<u16>()` で解析している箇所が `.unwrap_or(0)` で処理されるため、ユーザーが `${PORT:-99999}:3000` のようなミスをした場合にポート `0` としてサイレントに処理される。

```rust
// src/core/compose_parser.rs:355-356
default_port: caps[2].parse().unwrap_or(0),
container_port: caps[3].parse().unwrap_or(0),
```

**影響**: ユーザーの誤った設定を検出できず、意図しないポート割り当てが行われる。
**対応**: parse 失敗時にwarningを出力する。

### 1.3 `WORKTREE_PORT_STEP=0` のバリデーション未実施

`WORKTREE_PORT_STEP` が `0` の場合、全worktreeが同じポートを使用してしまい、ポート衝突の防止という本ツールの根本目的を達成できない。

```rust
// src/commands/init.rs:181-183
let port_step = env_file::get_var(&wt.path.join(".env"), "WORKTREE_PORT_STEP")
    .and_then(|v| v.parse::<u16>().ok())
    .unwrap_or(DEFAULT_PORT_STEP);
// port_step == 0 を検出していない
```

**対応**: `port_step >= 1` のバリデーションを追加する。

### 1.4 `env_file.rs` の HashMap アクセスで panic の可能性

```rust
// src/core/env_file.rs:94
let new = map[&old];  // old が map に存在しない場合 panic
```

URL内のポート置換処理中、`map[&old]` で直接インデックスアクセスしている。理論上は regex が `port_alts` に含まれるポートのみをキャプチャするため問題ないが、防御的プログラミングの観点から `.get()` を使用すべき。

### 1.5 コマンドハンドラのテスト完全欠如

| モジュール | 行数 | テスト |
|-----------|------|--------|
| `commands/init.rs` | 418 | **0** |
| `commands/status.rs` | 129 | **0** |
| `commands/hooks.rs` | 140 | **0** |
| `commands/env.rs` | 54 | **0** |
| `commands/cleanup.rs` | 30 | **0** |
| `commands/prune.rs` | 36 | **0** |
| `core/docker.rs` | 66 | **0** |
| `core/worktree.rs` | 98 | **0** |
| `core/shared_services.rs` | 67 | **0** |

dev-dependencies に `assert_cmd` と `predicates` があるが一切使用されていない。**807行の本番コードが未テスト** (全体の約37%)。特に `init.rs` はツールの中核ロジックであり、テストの欠如はリスクが高い。

**対応**: 少なくとも integration test で主要フローをカバーする。

---

## 2. 重要度: 中 — 改善を推奨する問題

### 2.1 `override_gen.rs` の YAML インジェクションリスク

サービス名、ボリューム名、ディレクティブ名がバリデーションなしで YAML に書き出される。

```rust
writeln!(out, "  {svc}:").unwrap();
writeln!(out, "    {}: \"service:{}\"", cr.directive, target_svc).unwrap();
```

サービス名に `\n`, `:`, `"` などの YAML 特殊文字が含まれる場合、不正な YAML が生成される可能性がある。Docker Compose が拒否するため実被害は低いが、エラーメッセージが不親切になる。

**対応**: サービス名が `^[a-zA-Z0-9_-]+$` に適合することを検証する。

### 2.2 `docker ensure_network()` のエラー握りつぶし

```rust
// src/commands/init.rs:309 付近
docker::ensure_network().ok()  // エラーを無視
```

Docker ネットワーク作成の失敗がユーザーに通知されない。Docker daemon が停止している場合など、デバッグが困難になる。

**対応**: 失敗時に warning を出力する。

### 2.3 `cleanup` コマンドが `.env` をクリーンアップしない

`wtc cleanup` はスロット登録と override ファイルを削除するが、`.env` に書き込まれたポート変数・`COMPOSE_PROJECT_NAME` はそのまま残る。再 init 時に古い値との混乱が生じる可能性がある。

### 2.4 `prune` コマンドが override ファイルをクリーンアップしない

削除済み worktree のスロットは除去されるが、`docker-compose.override.yml` は残存する。

### 2.5 `override_gen.rs` の `writeln!().unwrap()` 多用

`String` への `writeln!` は原理上失敗しないが、60箇所以上の `.unwrap()` はコード品質の観点から改善余地がある。`write!` マクロの結果を `?` で伝搬するか、`String` に直接 push する方式を検討。

### 2.6 プロジェクト名の重複ロジック

`init.rs`, `status.rs`, `env.rs` で COMPOSE_PROJECT_NAME の算出ロジックが重複している。共通関数への抽出を推奨。

### 2.7 Docker Compose v2 の前提が未文書化

`docker.rs` は `docker compose` サブコマンド（v2系）を前提としており、旧 `docker-compose` CLI には非対応。`depends_on: required: false` は Compose v2.20+ が必要。これらの前提条件が README に記載されていない。

### 2.8 IPv6 アドレス非対応

ポート検出の正規表現は IPv4 のみ対応 (`\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}`)。IPv6 の `[::1]:5432:5432` はマッチしない。使用頻度は低いが、制限事項として文書化すべき。

---

## 3. 重要度: 低 — あると良い改善

### 3.1 CLI の利便性向上

- `--verbose` / `--quiet` フラグがない (デバッグ情報の制御不可)
- `wtc status` にフィルタリングオプションがない
- `wtc env` に `--json` / `--shell` 出力形式オプションがない
- `wtc prune` に `--dry-run` オプションがない

### 3.2 Git エラー検出の脆弱性

```rust
// src/core/worktree.rs
// stderr に "not a git repository" が含まれるかで判定
```

Git のエラーメッセージ文字列にマッチしているため、Git のバージョンやロケール変更で動作が変わる可能性がある。

### 3.3 OSS標準ファイルの不足

- `LICENSE` — **未作成** (必須)
- `CHANGELOG.md` — 未作成 (推奨)
- `CONTRIBUTING.md` — 未作成 (推奨)
- `.gitignore` — `/target` のみ (最低限で問題なし)

### 3.4 slot_registry のバリデーション不足

- slot 0 が複数存在しうる (重複チェックなし)
- 重複パスのチェックなし
- Windows での `fs::rename()` のアトミック性が未検証

### 3.5 `shared_services.rs` のプロファイル操作

プロファイル名のカンマ区切り処理で、トリム後の結果に前後の空白が残る場合がある。

```rust
// 入力: "_wt_main_only, other"
// 削除後: " other" (先頭スペース残存)
```

---

## 4. セキュリティ評価

### 良い点

- **コマンドインジェクション対策**: 全ての外部コマンド実行が `Command::new().args()` を使用。シェル経由の実行なし
- **パストラバーサル対策**: パスは `canonicalize()` で正規化
- **ファイルロック**: `fs2` による排他ロックで TOCTOU レース回避

### 要注意点

- **YAML インジェクション**: override_gen でのサービス名未検証 (前述 2.1)
- **Docker stderr 非表示**: エラー診断困難 (前述 2.2)

---

## 5. テスト品質の詳細

### 良好なテスト (core/)

| モジュール | テスト数 | カバー範囲 |
|-----------|---------|-----------|
| `compose_parser.rs` | 51 | ポートパターン、YAML anchor、merge key、placeholder、IPv4、プロトコル |
| `env_file.rs` | 11 | get/set、URL置換、バッチ処理、引用符、cascading防止 |
| `override_gen.rs` | 8 | 基本生成、shared service、volume、container name |
| `slot_registry.rs` | 7 | 空レジストリ、割当、保存/再読込、削除、slot再利用 |
| `port.rs` | 4 | slot 0/1、overflow、shared service |

### テストが不足している領域

1. **統合テスト**: `assert_cmd` が dev-dependency にあるが未使用
2. **linked worktree の depends_on**: override_gen での `required: false` 生成
3. **同時実行テスト**: fs2 ロックの実際の排他動作
4. **エラーパステスト**: worktree 検出失敗、Docker 未インストール時の挙動
5. **境界値テスト**: ポート 65535 付近、slot 数が多い場合

---

## 6. 依存関係評価

全ての依存関係は最新メジャーバージョンで、セキュリティ上の懸念なし。

| 依存関係 | バージョン | 評価 |
|---------|-----------|------|
| `clap` | 4 | ★★★★★ 標準的な CLI パーサー |
| `anyhow` | 1 | ★★★★★ エラーハンドリングの定番 |
| `thiserror` | 2 | ★★★★★ ドメインエラー定義 |
| `serde_yaml_ng` | 0.9 | ★★★★★ YAML 1.2 準拠 (良い選択) |
| `fs2` | 0.4 | ★★★★☆ ファイルロック。安定版だがメンテ頻度低め |
| `regex` | 1 | ★★★★★ |
| `owo-colors` | 4 | ★★★★☆ 軽量ターミナル色付け |

依存数は9個と最小限。

---

## 7. 対応優先度まとめ

### OSS公開前に必須 (P0)

1. LICENSE ファイルの追加
2. `WORKTREE_PORT_STEP=0` のバリデーション追加
3. Docker Compose v2 前提の README 明記

### 早期に対応推奨 (P1)

4. ポート番号 > 65535 のwarning追加
5. `env_file.rs` の `map[&old]` を `map.get()` に変更
6. 主要フローの integration test 追加 (少なくとも `init` → `status` → `cleanup`)
7. `ensure_network()` のエラー通知改善

### 中期的に改善 (P2)

8. プロジェクト名算出ロジックの共通化
9. IPv6 非対応の文書化
10. `prune` での override ファイルクリーンアップ
11. YAML 生成時のサービス名バリデーション
12. `--verbose` / `--quiet` フラグの追加

### 将来的な検討 (P3)

13. `wtc env --json` 出力対応
14. `wtc prune --dry-run` オプション
15. CHANGELOG.md / CONTRIBUTING.md の整備
16. Windows サポートの検証・文書化
