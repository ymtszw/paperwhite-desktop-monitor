# Kindle Paperwhite Desktop Monitor

Kindle Paperwhite（第7世代・マンガモデル等）の「体験型ブラウザ」向けに作られた、PC・NAS・部屋の環境を表示するダッシュボードサーバーです。

Rust 製の単一バイナリで、Windows 上でコンソール画面を出さずにバックグラウンド常駐します。旧世代 WebKit と E-ink の制約に合わせてスクロールが発生しない固定レイアウトになっています。

---

## 主な機能

- **E-ink / 旧ブラウザ向け UI**:
  - 画面内に収まる固定 3×3 グリッド（スクロールなし）
  - 白黒高コントラスト表示（白黒反転・ダークモード切替対応）
  - 残像除去（画面クリア）機能
  - 画面回転（0° / 90° / 180° / 270°）
  - 数値の急変によるチラつきを抑える移動平均スムージング
- **PC スリープ検知 & 自動リロード**:
  - PC スリープ時は待機カード（☾ Zzz）に自動切替、復帰時に自動復帰
  - サーバー再起動時はクライアントが自動でリロード
- **監視項目**:
  - **PC**: CPU（使用率・履歴、ACPI/PDH 温度、コア数）、GPU（NVIDIA NVML 使用率・履歴、VRAM、温度、ファン RPM）、RAM、ネットワーク帯域（推移）、全ローカルドライブ使用量
  - **部屋の温湿度**: SwitchBot 温湿度計連携（温度、湿度、電池残量、60分履歴）
  - **NAS**: Synology 等の外部メトリクス（CPU 履歴、RAM、ボリューム、帯域、HDD 温度・SMART 状態）
  - **システム情報**: ホスト名、稼働時間、温度、IP
- **サーバー仕様**:
  - Windows GUI サブシステム（`#![windows_subsystem = "windows"]`）による完全ヘッドレス動作
  - 外部ランタイム（Python 等）不要、HTML/CSS/JS 内包の単一バイナリ
  - プライベート LAN 限定アクセス制御（外部 IP は 403 遮断）

---

## 画面構成 (3×3 Grid)

```
┌─────────────────┬─────────────────┬─────────────────┐
│     [ CPU ]     │     [ GPU ]     │     [ RAM ]     │
│ 使用率 & グラフ │ 使用率 & グラフ │ 使用率 & グラフ │
│ 温度 / コア数   │ VRAM / 温度/RPM │ 使用量 / 総容量 │
├─────────────────┼─────────────────┼─────────────────┤
│   [ NETWORK ]   │ [ ROOM CLIMATE ]│   [ STORAGE ]   │
│ 送受信速度/グラフ│ SwitchBot 温湿度 │ 全ローカルドライブ │
│                 │ 60分履歴/電池残 │ 空き容量バー    │
├─────────────────┼─────────────────┼─────────────────┤
│   [ NAS SYS ]   │  [ NAS VOL/NW ] │   [ SYSTEM ]    │
│ CPU履歴 / RAM   │ 容量 / NW速度   │ 画面回転 / クリア│
│ 温度 / ONLINE   │ HDD温度 / SMART │ ホスト情報 / 温度│
└─────────────────┴─────────────────┴─────────────────┘
```

---

## 使い方

### 1. ビルド

```powershell
cargo build --release
```

バイナリは `target/release/paperwhite-desktop-monitor.exe` に生成されます。

### 2. 設定 (`config.json`)

NAS や SwitchBot と連携する場合は、`config.example.json` をコピーして `config.json` を作成します（未設定の項目は自動でスタンバイ表示になります）：

```json
{
  "nas": {
    "statsUrl": "http://192.168.0.12:8789/api/stats"
  },
  "switchbot": {
    "token": "YOUR_SWITCHBOT_TOKEN",
    "secret": "YOUR_SWITCHBOT_SECRET",
    "deviceId": "YOUR_DEVICE_ID",
    "deviceName": "リビング温湿度計"
  }
}
```

### 3. 起動・停止

- **手動起動**: `start.bat` または `.\target\release\paperwhite-desktop-monitor.exe`
- **停止**: `stop.bat`
- **Windows 起動時の自動常駐**:
  - 登録: `setup_autostart.bat`（レジストリ `Run` キーに登録）
  - 解除: `remove_autostart.bat`

---

## Kindle Paperwhite 側の手順

1. Kindle を PC と同じ Wi-Fi に接続します。
2. 右上メニューから「体験型ブラウザ」を開きます。
3. アドレスバーに `http://<PCのIPアドレス>:28080/` を入力して開きます。
4. ブックマークに登録しておくと次回以降すぐに開けます。
5. （推奨）部屋の照明で十分な場合はフロントライトを 0 にすると見やすくなります。

---

## 開発情報

アーキテクチャ、センサー仕様、品質ゲート等の詳細は [AGENTS.md](AGENTS.md) を参照してください。

---

## ライセンス

[MIT License](LICENSE)
