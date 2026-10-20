<p align="center">
  <img src="docs/screenshots/hero.png" alt="Swapbar: açık ve koyu temada menü çubuğu popover'ı; paket etiketleri, 5h ve Fable kotaları ve token maliyetiyle dört Claude hesabı">
</p>

<p align="center">
  Tüm Claude hesaplarının kotası tek bakışta, hesap değiştirmek tek tık.<br>
  <a href="https://github.com/realiti4/claude-swap"><code>claude-swap</code></a> (cswap) için macOS menü çubuğu ve Windows sistem tepsisi uygulaması.
</p>

<p align="center">
  <a href="https://github.com/afatmustafa/swapbar/releases/latest"><img src="https://img.shields.io/github/v/release/afatmustafa/swapbar" alt="Latest release"></a>
  <a href="https://github.com/afatmustafa/swapbar/releases"><img src="https://img.shields.io/github/downloads/afatmustafa/swapbar/total" alt="Downloads"></a>
</p>

<p align="center">
  <a href="https://github.com/afatmustafa/swapbar/releases/latest"><b>İndir</b></a> ·
  <a href="https://swapbar.afat.me">Web sitesi</a> ·
  <a href="README.md">English</a> | <b>Türkçe</b>
</p>

<p align="center">
  <img src="docs/screenshots/popover.png" width="864" alt="Açık ve koyu temada Swapbar popover'ı: 5h, 7d ve Fable kota çubuklarıyla dört hesap ve token/maliyet tablosu">
</p>

Swapbar tüm cswap hesaplarının 5h, 7d ve model bazlı (Fable) kotalarını tek bakışta gösterir, tek tıkla hesap değiştirir ve Claude Code transkriptlerinden token ve maliyet özeti çıkarır.

## Özellikler

- **Kurulum asistanı.** İlk açılışta Swapbar, Claude Code ve claude-swap'ın kurulu olup olmadığına bakar, claude-swap eksikse `uv` ile kurar (gerekirse önce `uv`'yi de kurar) ve hesaplarınızı tek tek ekler: her hesap tarayıcıda `claude auth login` ile giriş yapar, cswap girişi saklar ve Claude Code kullandığı hesaba geri döner.
- **Tüm hesaplar tek bakışta. Her hesap için paketi (Max 20x, Max 5x, Pro, Team Premium/Standard), 5h, 7d ve model bazlı (Fable) çubuklar ve sıfırlanmaya kalan süre. Kalan %40'a inince çubuk sarıya, %15'e inince kırmızıya döner.
- **Tek tıkla geçiş.** Pasif bir hesaba tıklamak `cswap switch` çalıştırır. Aktif hesap **current**, otomatik rotasyon dışındaki hesaplar **paused** olarak işaretlenir.
- **Tempo uyarıları.** Bir hesap bir pencereyi çok hızlı harcıyorsa, çubuktaki işaret eşit tempoda nerede olması gerektiğini gösterir; pencere sıfırlanmadan bitecekse satır ne zaman biteceğini söyler. Aktif hesap için pencere başına bir bildirim gelir.
- **Geçiş önerisi. Aktif hesabın en dar penceresi düşük kota seviyesine (varsayılan %15 kalan) inince, en çok kotası kalan hesabı gösteren bir banner çıkar ve tek tıkla o hesaba geçilir. Bu hesap listede **suggested** olarak işaretlenir.
- **Bildirimler.** Aktif hesabın kotası azalınca en uygun hesaba geçmek için **Switch** butonlu bir bildirim, tükenmiş bir hesabın kotası sıfırlanınca da ayrı bir bildirim gelir.
- **Global kısayollar.** `⌃⌥S` her uygulamadan Swapbar'ı açar; `⌃⌥⇧S` en çok kotası kalan hesaba geçer. Erişilebilirlik izni gerekmez.
- **Güncelleme kontrolü.** Swapbar açılışta ve 6 saatte bir GitHub'da yeni sürüm arar; yeni sürüm varsa **Download** banner'ı gösterir.
- **Menü çubuğunda kota.** Swapbar ikonunun yanında aktif hesabın kotası `5h` ya da `5h · 7d` olarak, kalan veya kullanılan şeklinde görünür.
- **Token ve maliyet özeti.** Bugün ve son 30 gün için maliyet, token, istek, output ve cache read toplamları, hesap bazında (cswap'ın geçiş log'u, `cswap run` oturumları ve her profilin giriş yaptığı hesaptan çıkarılır) ve model bazlı maliyet dökümü ve input / cache read / cache write / output dağılım çubuğu.
- **Türkçe ve İngilizce.** macOS dilini izler; Ayarlar › Genel'den biri seçilebilir.
- **Açık ve koyu tema.** macOS görünümünü izler; Ayarlar'dan Açık ya da Koyu olarak sabitlenebilir.
- **Küçük ve yerel.** Rust ve [gpui](https://crates.io/crates/gpui) (Zed'in UI framework'ü). Apple Silicon ve Intel için tek universal binary, yaklaşık 4 MB.

## Ayarlar

Popover'daki **Settings…** ile açılır. Tercihler `~/.config/swapbar/settings.json` dosyasında tutulur.

<table>
  <tr>
    <td width="50%"><img src="docs/screenshots/settings-general-dark.png" alt="Koyu temada General ayarları"></td>
    <td width="50%"><img src="docs/screenshots/settings-menubar.png" alt="Menu Bar ayarları"></td>
  </tr>
  <tr>
    <td><b>General:</b> tema (System / Light / Dark), dil (Sistem / English / Türkçe), girişte başlat (LaunchAgent), yenileme aralıkları, kullanım bölümünü göster/gizle.</td>
    <td><b>Menu Bar:</b> yüzde modu (kalan / kullanılan), ikon yanındaki başlık (yalnız ikon / 5h / 5h · 7d), 7 gün penceresi, model limitleri, reset zamanları.</td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/settings-accounts.png" alt="Accounts ayarları"></td>
    <td><img src="docs/screenshots/settings-auto.png" alt="Auto-switch ayarları"></td>
  </tr>
  <tr>
    <td><b>Accounts:</b> hesap ekle, yeniden adlandır (<code>cswap alias</code>), sırala (<code>cswap swap</code>), bozuk girişi yenile, kaldır; her hesabı menüde göster/gizle, otomatik rotasyona dahil et (<code>cswap enable/disable</code>).</td>
    <td><b>Auto-switch:</b> cswap'ın <code>autoswitch.*</code> ayarları (<code>cswap config set</code>), varsayılanlara dönüş. Bu ayarlar yalnızca <code>cswap auto</code> çalışırken etkilidir.</td>
  </tr>
</table>

## Kurulum

### Homebrew

```sh
brew install --cask afatmustafa/tap/swapbar
```

Cask, Swapbar'ı Uygulamalar klasörüne kurar; `brew upgrade --cask swapbar` ile güncellenir. İlk açılışta kurulum asistanı gerekirse [`claude-swap`](https://github.com/realiti4/claude-swap)'ı kurar ve hesaplarınızı ekler.

### İndirme

1. [Son sürümden](https://github.com/afatmustafa/swapbar/releases/latest) `Swapbar.dmg` dosyasını indirin ve Swapbar'ı Applications klasörüne sürükleyin.
2. Swapbar'ı açın. İkonu menü çubuğunda belirir; claude-swap ya da hesaplar eksikse kurulum asistanı açılır. Hesaplara giriş için [Claude Code](https://claude.com/claude-code) gerekir.

Swapbar `cswap`'ı `~/.local/bin/cswap`, `/opt/homebrew/bin/cswap` ve `/usr/local/bin/cswap` konumlarında arar. Başka bir yol için `CSWAP_BIN` tanımlayın (`claude` ve `uv` için `CLAUDE_BIN` ve `UV_BIN` aynı şekilde çalışır).

0.1.1 ve sonraki sürümler Developer ID ile imzalı ve Apple tarafından notarize edilmiştir; Swapbar çift tıklayınca açılır. 0.1.0 öyle değildir: macOS ilk açılışı engeller, Sistem Ayarları → Gizlilik ve Güvenlik altındaki **Yine de Aç** ile izin verirsiniz.

Girişte başlatmak için Ayarlar'da **Launch at login** seçeneğini açın.

### Kaynaktan derleme

```sh
./scripts/bundle.sh                    # release derler, target/Swapbar.app üretir
./scripts/bundle.sh --universal        # arm64 + x86_64
./scripts/dmg.sh                       # paketten target/Swapbar.dmg üretir
cp -R target/Swapbar.app /Applications/
```

Gereksinimler:

- Rust toolchain
- Xcode. gpui `runtime_shaders` özelliğiyle derlenir, bu yüzden Metal toolchain gerekmez.

### Windows

Swapbar Windows 10 ve 11'de de sistem tepsisinde çalışır.

1. [Son sürümden](https://github.com/afatmustafa/swapbar/releases/latest) `Swapbar-windows-x64.zip` dosyasını indirin, istediğiniz yere açın ve `Swapbar.exe`'yi çalıştırın. Exe henüz kod imzalı olmadığı için SmartScreen önce sorabilir: **Ek bilgi › Yine de çalıştır**.
2. Windows 11 yeni tepsi ikonlarını saatin yanındaki **^** okunun altına koyar. Hep görünsün diye Swapbar'ı görev çubuğuna sürükleyin.

Kurulum asistanı aynı şekilde çalışır: claude-swap'ı `uv` ile kurar (`uv` yoksa onu da Astral'ın PowerShell betiğiyle). Swapbar `cswap.exe`'yi `%USERPROFILE%\.local\bin` içinde ve `PATH`'te, Claude Code'u da `claude.exe` ya da npm'in `claude.cmd`'si olarak arar.

Windows'ta farklı olanlar:

- **Tepsi ikonu.** Tepsi ikonunun yanında yazıya yer olmadığından, **İkondaki sayı** açıkken logonun yerine aktif hesabın 5h yüzdesi görünür. 5h ve 7d değerleri için üzerine gelin. Tıklayınca Swapbar açılır; sağ tıkta Aç, Ayarlar ve Çık vardır.
- **Bildirimler** tepsi ikonundan gelir ve henüz **Geç** butonu yoktur. Tıklayınca Swapbar açılır; geçiş önerisi orada tek tık uzaklıktadır.
- **Kısayollar** `Ctrl+Alt+S` ve `Ctrl+Alt+Shift+S`'dir.
- **Girişte başlat**, Swapbar'ı `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` anahtarına ekler. Ayarlar ve önbellekler `%APPDATA%\Swapbar` altındadır.

Kendiniz derlemek için Rust'ı ve Visual Studio Build Tools'u (MSVC ve Windows SDK) kurun, sonra:

```powershell
cargo build --release                  # target\release\swapbar.exe
./scripts/package-windows.ps1          # target\Swapbar-windows-x64.zip
python scripts/ico.py                  # AppIcon.icns'ten assets\AppIcon.ico (Pillow gerekir)
```

## Nasıl çalışır

| Veri | Kaynak |
|---|---|
| Hesaplar, 5h/7d/Fable kotaları | `cswap list --json`, 60 saniyede bir. Anthropic usage API'sini cswap kendisi çağırır. Bu endpoint hesap başına saatte yaklaşık 30 istekle sınırlı olduğu için sorgu sıklığını cswap yönetir. |
| Hesap değiştirme | `cswap switch <n> --json` |
| Token ve maliyet | `~/.claude/projects/**/*.jsonl`, ayrıca `CLAUDE_CONFIG_DIR`, `~/.claude-*` profilleri ve cswap oturum dizinleri. Kayıtlar `message.id:requestId` ile tekilleştirilir, 2 dakikada bir artımlı taranır. |
| Fiyatlar | Gömülü tablo ve üzerine [models.dev](https://models.dev/api.json) verisi (24 saat önbellek). 1h cache write = 2× input, 5m cache write = 1.25× input. |

Önbellekler `~/.config/swapbar/` altındadır (`usage-cache.json`, `model-rates.json`).

Maliyet rakamları API liste fiyatlarına göre tahmindir, abonelik faturanız değildir.

Swapbar Anthropic'e doğrudan bağlanmaz. Kotalar cswap'tan gelir, kullanım Mac'inizdeki transkriptlerden okunur.

## Geliştirme

```sh
cargo run                                          # menü çubuğundaki Swapbar ikonuna tıklayın
SWAPBAR_OPEN=1 cargo run                           # popover'ı açılışta göster
SWAPBAR_SETTINGS=1 SWAPBAR_PAGE=about cargo run    # ayar penceresini belirli sayfada aç
SWAPBAR_DEBUG=1 cargo run                          # geometri ve tarama logları
cargo run -- --scan                                # GUI'siz: token ve maliyet özetini yazdır
./scripts/icon.sh                                  # assets/AppIcon.icns'i AppIcon.svg'den yeniden üret
```

`SWAPBAR_PAGE` şu değerleri alır: `menubar`, `accounts`, `auto`, `about`. Diğer değerler General sayfasını açar.

Popover ve ayar ekran görüntülerinde demo hesaplar ve üretilmiş transkriptler kullanıldı. Üstteki görseldeki kullanım ve kota rakamları temsilidir.

### Dosyalar

- `src/main.rs`: uygulama durumu, zamanlayıcılar, pencere açma/kapama, menü çubuğu başlığı
- `src/statusbar.rs`: NSStatusItem (cocoa), tıklama geri çağrısı, ekran geometrisi
- `src/cswap.rs`: cswap köprüsü (`list`, `switch`, `add`, `remove`, `alias`, `swap`)
- `src/setup.rs`: `claude`/`cswap`/`uv`/`brew` tespiti, claude-swap kurulumu, `claude auth login`
- `src/onboarding.rs`: kurulum asistanı penceresi
- `src/i18n.rs`: İngilizce/Türkçe metinler (`l()`, `lf!`) ve cswap zaman metinlerinin Türkçesi
- `src/dialog.rs`: native NSAlert onay ve metin girişi
- `src/advice.rs`: geçiş önerisi, tempo, bildirim olayları
- `src/usage.rs`: JSONL tarayıcı, önbellek, dönem özetleri
- `src/pricing.rs`: model fiyatları
- `src/assets.rs`: gömülü SVG ikonlar (AssetSource)
- `src/theme.rs`: açık/koyu görünüm
- `src/ui.rs`: popover arayüzü
- `src/settings.rs`: tercih modeli, LaunchAgent
- `src/settings_ui.rs`: ayar penceresi ve About sayfası
- `src/win/`: `statusbar`, `dialog`, `notify`, `hotkey` ve `theme` modüllerinin Windows sürümleri (tepsi ikonu, görev diyalogları, tepsi bildirimleri, RegisterHotKey) ve bunların mesajlarını alan gizli pencere
- `build.rs`, `assets/windows/swapbar.rc`: Windows'ta exe'nin ikonu ve sürüm bilgisi

### Web sitesi

Tanıtım sayfası ayrı bir repoda duruyor: `swapbar-website`.

## Katkılar

İkonlar [Hugeicons](https://hugeicons.com/?via=afat) stroke-rounded (MIT, `assets/icons/` altında). Hesap verisi ve geçiş [claude-swap](https://github.com/realiti4/claude-swap) üzerinden.

[Mustafa Afat](https://afat.me) tarafından yapıldı.
