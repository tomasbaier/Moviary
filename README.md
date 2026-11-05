# Moviary

Aplikace pro Windows a Android (Tauri 2 – Rust + webový frontend) pro kategorizaci a přehrávání filmů
a seriálů ve VLC. Obě verze sdílejí stejný kód; liší se jen přehrávačem a výběrem složek.

> **English:** Moviary organises the movies and series in your folders into your own categories,
> plays them in VLC and resumes where you left off. Posters, overviews and cast come from TMDB with
> your own free API key. The app is available in Czech and English (switch in the top bar).

## Stažení

Hotové soubory jsou v sekci [Releases](../../releases):

- `Moviary-<verze>-windows.exe` – jeden soubor, nic se neinstaluje. Potřebuje Windows 10/11
  a [VLC](https://www.videolan.org/).
- `Moviary-<verze>-android.apk` – Android 7+; potřebuje VLC pro Android z Google Play.
  Instalace mimo obchod je potřeba v telefonu povolit („Instalovat neznámé aplikace“).
- `Moviary-<verze>-third-party-licenses.txt` – licence použitých knihoven.

Exe není podepsané certifikátem, takže Windows při prvním spuštění zobrazí varování SmartScreen
(„Systém Windows ochránil váš počítač“). Spustíte ho přes **Další informace → Přesto spustit**.

Pro plakáty a popisy si vytvořte bezplatný API klíč na
[themoviedb.org](https://www.themoviedb.org/settings/api) a vložte ho v Nastavení.

## Funkce

- Více složek s filmy (i na externích discích) – rekurzivně najde video soubory (mkv, mp4, avi, mov,
  wmv, m4v, ts, webm…), z názvu souboru odvodí název filmu a rok. Filmy z odpojeného disku se
  zobrazí zašedle jako „Nedostupné“.
- Spolehlivé rozpoznání souborů: data jsou vázaná na složku + relativní cestu a na otisk obsahu
  (velikost + hash tří 64KB vzorků), takže přežijí přejmenování a přesun souboru i změnu písmena
  disku (u složky stačí „Změnit umístění“).
- Informace z TMDB (s bezplatným klíčem): plakát, český název, popis, žánry, hodnocení, režie,
  délka; žánry se mohou automaticky přidat jako kategorie. Chybnou shodu lze opravit ručním
  vyhledáním. Filmy bez plakátu dostanou náhled – snímek z videa vytvořený pomocí VLC.
- Seriály v samostatné záložce, rozdělené do sezón. Epizoda se pozná podle názvu souboru
  (`Přátelé S01E02`, `Přátelé 1x02`, i `S01E01E02`) nebo podle složky sezóny
  (`Přátelé\Sezóna 1\02 - Název.mkv`, také `Season 1`, `Série 1`, `2. řada`, `S01`, `Speciály`).
  Stránka seriálu ukazuje sezóny se seznamem epizod, tlačítko „Pokračovat“ na další díl,
  označení celé sezóny jako zhlédnuté a s TMDB klíčem i názvy a popisy epizod a chybějící díly.
  Kategorie se přiřazují celému seriálu.
- Vlastní kategorie: vytváření, přejmenování, mazání; přiřazení k jednomu filmu (detail vpravo)
  nebo hromadně k více vybraným filmům (Ctrl/Shift + klik).
- Filtrování podle kategorií (kterákoli / všechny vybrané), „Bez kategorie“, stavu
  (rozkoukané / neviděné / zhlédnuté) a fulltextové hledání (bez ohledu na diakritiku).
- Přehrávání ve VLC. Aplikace sleduje pozici přes lokální HTTP rozhraní VLC (náhodný port a heslo,
  jen 127.0.0.1) a ukládá ji každých 5 s i při zavření VLC. Příště film naváže na uloženou
  pozici (o 5 s dříve). Po dokoukání (≥ 95 % nebo posledních 30 s) se film označí jako zhlédnutý.
- Technické údaje přímo z hlaviček souborů MKV/WebM a MP4/MOV (čte se jen pár set kB, ne celý
  soubor): kodek, rozlišení, HDR10/HLG/Dolby Vision, bitová hloubka, délka a zvukové a titulkové
  stopy s jazyky a počtem kanálů; k tomu titulky uložené vedle filmu (`Film.cs.srt`, složka
  `Subs`). U jiných formátů (AVI…) se údaje odhadnou z názvu (`2160p`, `x265`, `HDR`, `CZ dabing`,
  `CZ titulky`, `5.1`). Čtečka je v [src-tauri/src/media.rs](src-tauri/src/media.rs).
- Technické filtry: rozlišení, HDR, kodek, jazyk zvuku, prostorový zvuk, jazyk titulků / bez titulků.
- Zvuk a titulky při přehrávání (Windows) ve třech úrovních – konkrétnější vyhrává:
  1. globální pravidla v Nastavení: preferované jazyky zvuku a titulky „podle souboru / vypnuté /
     vždy / jen u cizojazyčného zvuku“ (jinak jen vynucené titulky) s preferovanými jazyky titulků;
  2. u seriálu jazyk zvuku a titulků pro všechny epizody;
  3. u filmu konkrétní zvuková stopa a titulková stopa / soubor / vypnuto.
  Detail filmu ukazuje, co se přehraje a podle čeho. Výsledek se předá VLC (`--audio-track`,
  `--sub-track`, `--sub-file`, `--sub-language=none`), titulky jdou ve VLC dál přepínat.
  VLC pro Android stopy z aplikace vybrat neumí, řídí se svým vlastním nastavením jazyků.
- Statistiky: počty, velikost a délka knihovny, nakoukané hodiny po měsících (z historie
  přehrávání delších než minuta), kategorie, žánry, desetiletí, rozlišení, jazyky, složky,
  naposledy přehrané a nejlépe hodnocené.
- Duplicity: stejné soubory (otisk), stejný film podle TMDB, stejná epizoda a stejný název s rokem;
  ukáže, kolik místa by šlo uvolnit. Nic nemaže.
- Záloha a obnova: export do souboru, automatická denní záloha (posledních 7 ve složce `backups`),
  obnovení ze zálohy (současný stav se předtím zálohuje). Plakáty se po obnovení stáhnou znovu.
- Úvodní řady nad seznamem (když není zapnutý filtr): „Pokračovat ve sledování“ (rozkoukané filmy
  a další díly seriálů), „Nově přidané“ a „Neviděné z oblíbených kategorií“. Nové filmy mají
  několik dní štítek „Nový“ (kromě prvního načtení knihovny). Řady jdou v Nastavení vypnout.
- Herci a štáb z TMDB: obsazení a režie (u seriálů tvůrci) v detailu; kliknutím se vyfiltrují
  všechny filmy a seriály daného člověka, hledat jde i podle herců.
- Filmové série (kolekce TMDB): díly v pořadí s tím, co je v knihovně, co je zhlédnuté a co chybí,
  tlačítko „Pokračovat dalším dílem“ a filtr celé série.
- Podobné ve vaší knihovně: doporučení TMDB omezená na tituly, které máte.
- „Co dnes?“: náhodný neviděný film (seriál) z aktuálního výběru.
- Hlídání složek: nové, přesunuté a smazané soubory se projeví samy (po několika sekundách klidu,
  takže kopírování velkého souboru vyvolá jedno prohledání), stejně jako připojení nebo odpojení
  disku (kontrola každých 30 s). Lze vypnout v Nastavení.
- Starší informace z TMDB (bez herců, sérií a doporučení) se doplní na pozadí podle známé shody.
  Stejně se obnoví údaje a plakáty starší než 150 dní – podmínky TMDB nedovolují ukládat jejich
  data déle než 6 měsíců (obnova běží s automatickým načítáním informací a s nastaveným klíčem).
- Chci vidět, vlastní hodnocení a poznámka: u filmu i seriálu (v detailu, na stránce seriálu nebo
  hromadně pro vybrané). Stav „Chci vidět“ ve filtrech a řada na úvodní obrazovce, řazení „podle mého
  hodnocení“ (1–5 ♥), hledá se i v poznámkách.
- Nové díly seriálů: s TMDB klíčem ukazuje u běžícího seriálu datum dalšího dílu („Další díl S02E05
  vyjde čt 12. 11.“) a na úvodní obrazovce řadu „Nové díly seriálů“ (vyšlé za poslední týden a
  chystané na příští měsíc). Údaje o běžících seriálech se obnovují jednou týdně a den po vydání dílu.
- Uložené filtry: libovolnou kombinaci filtrů (záložka, stav, kategorie, technické údaje, složka,
  herec, hledaný text a řazení) jde uložit pod názvem do postranního panelu a jedním klikem obnovit.
- Aktualizace: jednou denně se podívá do [GitHub Releases](../../releases), jestli nevyšla nová verze,
  a nabídne ji i s poznámkami k vydání. Na Windows ji stáhne, ověří (velikost a SHA-256 z GitHubu),
  nahradí jí běžící exe a restartuje se – zástupci dál fungují. Na Androidu stáhne APK a otevře
  systémovou instalaci (napoprvé je potřeba povolit „Instalovat neznámé aplikace“). Verzi lze
  přeskočit, kontrolu vypnout v Nastavení. Nové vydání musí mít tag `v<verze>` a soubory
  `Moviary-<verze>-windows.exe` a `Moviary-<verze>-android.apk`.
- O aplikaci (ikona ⓘ v horní liště, nebo z Nastavení): verze, upozornění TMDB s logem, VLC,
  soukromí a licence všech použitých knihoven.
- Čeština a angličtina: přepínač v horní liště (CS / EN) a v Nastavení. Při prvním spuštění
  rozhodne jazyk systému (čeština a slovenština → česky, jinak anglicky). Mění texty, formát čísel
  a dat i hlášky backendu a Android pluginu. Informace z TMDB (názvy, popisy, žánry, názvy epizod)
  se stahují česky i anglicky a zobrazují se v jazyce aplikace; u údajů načtených starší verzí se
  angličtina doplní na pozadí (potřeba TMDB klíč). Kategorie zůstávají, jak je máte. Texty jsou v kódu česky, anglické překlady ve slovníku [src/i18n.js](src/i18n.js)
  (`t('Český text')`, Rust: makro `tr!` v [src-tauri/src/i18n.rs](src-tauri/src/i18n.rs)).

Data se ukládají do `%APPDATA%\cz.filmoteka.desktop\library.json` (identifikátor pochází z původního
názvu Filmotéka a zůstává kvůli kompatibilitě s daty starších verzí), plakáty a náhledy do
podsložky `posters`, automatické zálohy do `backups`. Data starších verzí se při prvním spuštění
automaticky převedou.

## Android

Funguje stejně jako na Windows: přidáte složky s filmy a seriály (v paměti telefonu, na SD kartě
nebo USB disku), aplikace je prohledá, roztřídí a přehrává ve **VLC pro Android**.

- Při prvním přidání složky si aplikace řekne o „Přístup ke všem souborům“ (Android 11+) – videa
  jen čte, nic nemění. Složka se vybírá systémovým výběrem složek.
- Film se otevře ve VLC pro Android (od uložené pozice, o 5 s dříve). Po zavření VLC si Moviary
  převezme pozici, kde jste skončili; dokoukaný film se označí jako zhlédnutý. VLC musí mít také
  povolený přístup k souborům (zeptá se samo), titulky vedle videa najde automaticky.
- Náhledy filmů bez plakátu vytváří systémový dekodér Androidu (MediaMetadataRetriever).
- Na telefonu jsou filtry ve vysouvacím panelu (ikona trychtýře), detail filmu přes celou
  obrazovku, hromadný výběr dlouhým podržením karty; systémové tlačítko Zpět zavírá detail,
  stránku seriálu, výběr i dialogy.
- Data jsou v soukromé složce aplikace (`/data/data/cz.filmoteka.app/`), nesdílejí se s Windows.
- USB disky Android aplikacím většinou nezpřístupní jako soubory, jen přes systémové rozhraní
  dokumentů (SAF). Takovou složku si Moviary uloží s trvalým oprávněním a prochází ji přes SAF;
  VLC dostane `content://` adresu souboru (titulky vedle videa pak VLC samo nenajde). Složky
  v paměti telefonu a na SD kartě se čtou přímo jako soubory.
- Síťová úložiště (SMB/NAS) nejsou podporovaná.

Android část je v `src-tauri/src/mobile.rs` (Rust) a v Kotlin pluginu
`src-tauri/gen/android/app/src/main/java/cz/filmoteka/app/FilmotekaPlugin.kt`.

## Požadavky

- Windows 10/11 s WebView2 (ve Windows 11 je součástí systému)
- [VLC](https://www.videolan.org/) – hledá se v `Program Files` a v registru, cestu lze nastavit ručně.

## Sestavení

```bash
npm install
npm run build
```

Výsledek: `src-tauri/target/release/moviary.exe` (jeden samostatný soubor, frontend je uvnitř),
zkopírovaný i do `Release`.

Vývojový režim: `npm run dev`.

### Android

Potřeba: Android Studio (SDK + JDK v `Android Studio\jbr`), Android NDK a Rust targety:

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
```

Proměnné prostředí (PowerShell, upravte verzi NDK):

```powershell
$env:JAVA_HOME = "C:\Program Files\Android\Android Studio\jbr"
$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
$env:NDK_HOME = "$env:ANDROID_HOME\ndk\29.0.14206865"
```

Sestavení APK:

```bash
npm run android:build
```

Skript [scripts/android-build.js](scripts/android-build.js) si JDK, SDK i NDK najde sám (proměnné
výše jsou potřeba jen při jiném umístění). Režim pro vývojáře ve Windows není potřeba – bez něj
skript knihovny do Android projektu zkopíruje místo symbolického odkazu.

Při sestavení vytvoří [scripts/licenses.js](scripts/licenses.js) z `cargo metadata` soubor
`src/third-party-licenses.txt` se seznamem a licencemi všech Rust knihoven Windows i Android
verze (zobrazí se v aplikaci v okně O aplikaci a zkopíruje se i do `Release`). Ručně: `npm run licenses`.

Výsledky buildů se kopírují do složky `Release` (`Moviary-<verze>-windows.exe`,
`Moviary-<verze>-android.apk`). APK je podepsané vlastním klíčem
`src-tauri/gen/android/keystore/filmoteka-release.jks` (heslo v `keystore.properties`); obojí
zálohujte a nesdílejte – bez stejného klíče nepůjde novou verzi nainstalovat přes starou.
Na připojeném telefonu nebo emulátoru: `npm run android:dev`.

## TMDB

<img src="src/assets/tmdb-logo.svg" alt="TMDB" height="16">

This application uses TMDB and the TMDB APIs but is not endorsed, certified, or otherwise
approved by TMDB.

Informace o filmech a seriálech (názvy, popisy, plakáty, herci, hodnocení) pocházejí z
[The Movie Database (TMDB)](https://www.themoviedb.org/). Aplikace s TMDB nemá žádnou smlouvu
a nemá vlastní klíč – každý uživatel používá svůj bezplatný API klíč a řídí se
[podmínkami TMDB API](https://www.themoviedb.org/api-terms-of-use). Aplikace je zdarma,
bez reklam a nekomerční.

## Soukromí

Moviary nemá účty, reklamy ani telemetrii. Připojuje se jen k `api.themoviedb.org` a
`image.tmdb.org` (s vaším klíčem) a jednou denně k `api.github.com` kvůli kontrole nových verzí
(v Nastavení lze vypnout). Ostatní data – kategorie, pozice přehrávání, historie –
zůstávají v datovém souboru v počítači nebo telefonu. VLC běží jako samostatný program; jeho
ovládací rozhraní poslouchá jen na `127.0.0.1` s náhodným heslem.

## Licence

Moviary je pod licencí [MIT](LICENSE). Použité knihovny mají vlastní licence (MIT, Apache-2.0,
MPL-2.0 a další), viz okno O aplikaci nebo `third-party-licenses.txt` u každého vydání.
VLC je ochranná známka VideoLAN, logo TMDB je ochranná známka TMDB; Moviary s nimi nijak
nesouvisí.
