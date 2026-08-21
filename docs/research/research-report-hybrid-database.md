# SemanticDB — Technisches Detailkonzept & Namensrecherche

## TL;DR
- **SemanticDB ist als in Rust geschriebener, embedded, EU-souveräner Hybrid-Datenspeicher technisch realistisch und mit reifen Open-Source-Crates umsetzbar** — Kern auf Apache Arrow + Lance-Format, Vektor-Schicht (HNSW/IVF-PQ) mit `multilingual-e5-small` (384 Dim, 100 Sprachen, CPU/ARM-tauglich), BM25 via Tantivy, RRF-Fusion, lokales LLM (Qwen) für NL→IR, CRDT-Sync via Loro, Python-Bindings via PyO3/maturin. Realistischer Aufwand für einen Einzelentwickler: **17–21 Personenmonate**.
- **Der USP** ist die Kombination aus (a) kindgerechter natürlichsprachlicher Abfrage über eine validierte Zwischen-IR mit Vorschau, (b) echter local-first-Souveränität inkl. AES-256-GCM-Verschlüsselung-at-rest und komplett offline arbeitendem LLM, und (c) einem einzigen embedded Artefakt, das exakte Filter + Semantik + Volltext fusioniert — diese Kombination bietet weder LanceDB (kein NL-/UX-Layer, kein CRDT-Sync) noch Qdrant (Server, ~400 MB RAM, nicht embedded-first) noch ein DuckDB+LLM-Wrapper (kein natives HNSW-Erststurm, kein Sandbox-IR) in einem Paket.
- **Namensempfehlung: „EuleDB"** ist der stärkste Kandidat (niedrigstes Kollisionsrisiko, DACH- + Weisheits-Assoziation, kindgerecht, „EU" unterstreicht Souveränität). **„KlaroDB"** kollidiert mit dem etablierten OSS-Consent-Manager „Klaro!" (npm-Name belegt) und **„Fraga"** mit einer etablierten automotive Datenbank-Firma; beide meiden. **„Semba"** (PyPI belegt) und **„Simsala"** (npm belegt, Pokémon-Assoziation) sind mittleres Risiko.

## Key Findings

1. **Der Rust-Stack existiert und ist reif.** Alle Kernbausteine sind produktionsnahe Open-Source-Crates: `lance`/`arrow-rs`, `tantivy`, `roaring`, `aes-gcm`, `loro`, `pyo3`/`maturin`. Lance bringt bereits selbst Hybrid-Search (Vektor + BM25 + SQL auf demselben Dataset) mit — das reduziert Eigenentwicklung erheblich, wirft aber die strategische Frage auf, wie viel man selbst baut vs. auf Lance aufsetzt.
2. **Das multilinguale Embedding-Problem ist gelöst.** `intfloat/multilingual-e5-small` (laut Hugging-Face-Model-Card 12 Layer, Embedding-Größe 384, initialisiert aus `microsoft/Multilingual-MiniLM-L12-H384`, unterstützt 100 Sprachen von XLM-RoBERTa, max. 512 Token) läuft nachweislich auf CPU/ARM (im Vespa-Test auf M1 Pro/arm64) — ideal als Default für M4 **und** Pi 5.
3. **Das lokale LLM ist der Engpass auf dem Raspberry Pi 5.** Qwen 7–8B ist auf dem Pi 5 unrealistisch (mehrere unabhängige Community-Benchmarks 2025/26: 7B < 2 tok/s, teils < 1 tok/s). Praktikabel sind auf dem Pi 5 nur 1–3B-Modelle @ Q4_K_M (ca. 2–5 tok/s). Auf dem MacBook M4 laufen 4–8B-Modelle via MLX/llama.cpp flüssig. Konsequenz: gestaffelte Modellwahl + regelbasierter Fallback-Parser.
4. **Sicherheit ist über validierte IR lösbar.** Der Standardansatz — das LLM erzeugt nie direkt ausführbaren Code, sondern eine typisierte Zwischen-IR, die deterministisch validiert und in read-only-Pläne übersetzt wird — ist Best Practice in der Text-to-SQL-Forschung (IRNet/NatSQL entfernen schwer alignbare Konstrukte) und passt exakt zum Konzept.
5. **Namensrisiken sind real und messbar.** KlaroDB und Fraga kollidieren mit bestehenden Marken/Produkten; EuleDB ist am saubersten.

## Details

### 1. Schichtenarchitektur

**Schicht 0 — Speicher-/Format-Kern (Rust).**
- **Format:** Apache Arrow (`arrow-rs`) als In-Memory-Repräsentation; **Lance** als On-Disk-Format. Lance ist ein modernes Spaltenformat, das laut Lance-Projekt „100x faster than Parquet or Iceberg for random access without sacrificing scan performance" bietet — ein LanceDB-eigener Benchmark („Benchmarking random access in Lance") korrigiert das sogar nach oben („it's really more like 2000x"). Dazu Zero-Copy, automatische Versionierung und native Vektor-Indizes. **Design-Entscheidung:** SemanticDB setzt auf Lance als Storage-Layer statt ein eigenes Format neu zu bauen (spart geschätzt 3–5 PM).
- **Kompression:** `zstd` für Blöcke; **FSST** für String-Spalten (Lance nutzt FSST/Dictionary-Encoding bereits intern; im Crate-Ökosystem u.a. über das `#fsst`-Keyword vorhanden).
- **Indizes:** **ART (Adaptive Radix Tree)** für Punkt-/Range-Lookups auf Schlüsseln (Crates: `art-tree`/`art-rs` von Lagrang, basierend auf Leis/Kemper/Neumann 2013; alternativ eine SIMD-optimierte ART-Implementierung); **Roaring Bitmaps** (`roaring`, der offizielle RoaringBitmap/roaring-rs-Port) für Filter-Prädikate und Set-Operationen.
- **Verschlüsselung at rest:** `aes-gcm` (AES-256-GCM). Laut RustCrypto-README hat das Crate „one security audit by NCC Group, with no significant findings" (finanziert von MobileCoin) erhalten und nutzt AES-NI/CLMUL-Hardwarebeschleunigung auf x86 sowie konstantzeitfähige Implementierungen. Schlüsselableitung via Argon2id aus Passphrase; Envelope-Encryption mit rotierbarem Data-Encryption-Key.

**Schicht 1 — Semantische & lexikalische Suche.**
- **Vektor:** HNSW für kleine/mittlere Datenmengen (Crates: `hnsw_rs`, `hnswlib-rs`, `rust-cv/hnsw`; empfohlene Defaults laut rust-cv/hnsw M≈12–16, M0 = 2×M), IVF-PQ für große Sammlungen und speicherarme Ziele (Pi 5). Lance bringt IVF-PQ nativ mit. Default-Distanz: Cosine.
- **Volltext:** **Tantivy** (Lucene-inspiriert, reines Rust). Laut offiziellem README: „Configurable tokenizer (stemming available for 17 Latin languages) … Tiny startup time (<10ms) … BM25 scoring (the same as Lucene)"; CJK-Support über Drittanbieter (tantivy-jieba/cang-jie, lindera). Deckt die lexikalische Hälfte der Hybrid-Suche ab.
- **Embeddings:** Default-Modell `multilingual-e5-small` (ONNX, 384 Dim). Inferenz via `ort` (ONNX Runtime) oder Candle. **Auto-Embedding-Spalten:** bei INSERT wird eine deklarierte Textspalte transparent embeddet und im Vektor-Index abgelegt.

**Schicht 2 — Hybrid-Query-Planer.**
- Fusion via **Reciprocal Rank Fusion (RRF)**, Formel `score(d) = Σ_r 1/(k + rank_r(d))`. Originalquelle: Cormack, Clarke & Büttcher, „Reciprocal Rank Fusion outperforms Condorcet and individual rank learning methods" (SIGIR 2009), wo k=60 als über viele Benchmark-Datensätze robust gezeigt wurde; k=60 ist heute Default u.a. bei Elasticsearch (rrf-Retriever), OpenSearch, Weaviate, Qdrant und Azure AI Search. Für kleine Korpora (<100 Dok.) empfiehlt die Praxis k=10–20 (RRF ist score-skalenunabhängig, daher robust gegenüber inkompatiblen BM25-/Cosine-Skalen). Exakte Filter (ART/Roaring) werden als Pre-Filter angewandt, danach werden Vektor- + BM25-Kandidaten fusioniert; optionaler Cross-Encoder-Reranker als späteres Feature.

**Schicht 3 — NL→IR Abfrage-Schicht (lokales LLM).**
- Pipeline: **NL → (LLM) → typisierte IR → Validator → Plan → Fusion → Ergebnis + Erklärung in einfacher Sprache**.
- Das LLM erzeugt **nie** ausführbaren Code/SQL, sondern eine eng typisierte, serialisierbare **Query-IR** (z.B. serde-JSON mit Enum-Varianten: `Filter`, `SemanticSearch`, `FullText`, `Sort`, `Limit`). Vorbild: der IRNet/NatSQL-Ansatz aus der Text-to-SQL-Forschung, der schwer alignbare SQL-Konstrukte entfernt/normalisiert und so die „semantische Barriere" zwischen Nutzerabsicht und Maschinen-Query überbrückt.
- LLM-Laufzeit: **llama.cpp** (GGUF) plattformübergreifend; **MLX** auf Apple Silicon. Modellwahl gestaffelt (s.u.).

**Schicht 4 — CRDT-Sync (local-first).**
- **Loro** (Rust-CRDT-Bibliothek mit Rust-, JS/WASM- und Swift-APIs; Fugue-Text-Editing, bewegbare Bäume/Listen, LWW-Map, Delta-Updates, Shallow Snapshots, schnelles Dokumenten-Laden). Für Multi-Device-Sync ohne Server; P2P oder über beliebigen Transport.

**Schicht 5 — Python-Bindings.**
- **PyO3** + **maturin** (abi3-Wheels für breite CPython-Kompatibilität), `pyo3-arrow` für Zero-Copy-Arrow-Austausch zu Pandas/Polars/PyArrow.

### 2. Datenmodell
- **Tabellen** mit typisierten Spalten (Arrow-Schema). Zusätzlich **Auto-Embedding-Spalten**: deklarativ (`text_col → embeds into vec_col`); Pipeline embeddet bei Insert/Update automatisch.
- **Embedding-Pipeline:** Chunking (max. 512 Token wg. E5-Limit) → Prefix-Konvention (E5 erwartet `query:` bzw. `passage:`) → Embedding → L2-Normalisierung → HNSW/IVF-PQ-Insert.
- **Modellvorschlag:** Default `multilingual-e5-small` (384 Dim) für 16-GB-M4 **und** Pi 5. Optionales Qualitäts-Upgrade auf `multilingual-e5-base`/`-large` auf dem M4. Auf dem Pi 5 bleibt „small" die einzige praktikable Wahl.

### 3. Sicherheitskonzept
- **Memory Safety:** Rust-Kern, `#![forbid(unsafe_code)]` wo möglich; unsafe nur gekapselt in Index-Hotpaths.
- **Verschlüsselung:** AES-256-GCM at rest; optional verschlüsselte Sync-Deltas.
- **Capability-Tokens:** Zugriff auf Tabellen/Spalten über signierte Capability-Tokens (read/write/schema); Default **read-only**.
- **LLM-Sandbox:** LLM-Ausgabe → nur validierte IR wird akzeptiert; Validator lehnt unbekannte Felder/Operationen ab (fail-closed). Keine freie Code-Ausführung, kein DROP/DELETE aus dem NL-Pfad per Default (analog zu Pre-/Post-Safety-Checks in produktiven Text-to-SQL-Engines).
- **Audit-Log:** append-only, hash-verkettet; protokolliert IR, Plan, betroffene Zeilenzahl.
- **Prompt-Injection-Härtung:** Nutzerdaten in Ergebnissen werden nie als Instruktion an das LLM zurückgegeben (strikte Trennung Instruktion/Daten), gemäß den in der Text-to-SQL-Sicherheitsforschung (OWASP-LLM-Top-10 / P2SQL) beschriebenen Risiken.

### 4. Kindgerechte UX
- **Drei Modi:** (a) natürliche Sprache („Zeig mir alle Bilder mit Katzen von letztem Sommer"); (b) **Blockly-artige Bausteine** (visuelle Filter-/Such-Blöcke, die 1:1 auf die IR-Enum-Varianten mappen — der visuelle Editor produziert direkt valide IR, nicht Freitext); (c) Vorschau/Rückfrage in einfacher Sprache („Ich suche nach … Stimmt das?").
- Ergebnis-Erklärung immer in einfacher Sprache; Fehlermeldungen freundlich und ohne Fachjargon. Der Blockly-Modus ist zugleich die sicherste Eingabeart (keine LLM-Unsicherheit) und ein didaktisches Onboarding zur natürlichen Sprache.

### 5. Roadmap (Einzelentwickler, PM = Personenmonate)
- **P0 – Fundament (2–3 PM):** Arrow/Lance-Storage, Schema, Insert/Scan, zstd/FSST, AES-GCM, PyO3-Grundgerüst.
- **P1 – Indizes & exakte Queries (2–3 PM):** ART, Roaring, Filter-Planer, read-only-Capabilities, Audit-Log.
- **P2 – Semantik & Volltext (3–4 PM):** Embedding-Pipeline (E5/ONNX), HNSW/IVF-PQ, Tantivy-BM25, RRF-Fusion.
- **P3 – NL→IR-Layer (3–4 PM):** IR-Schema, Validator, llama.cpp/MLX-Anbindung, Qwen-Prompting, Vorschau/Erklärung, regelbasierter Fallback.
- **P4 – CRDT-Sync (2–3 PM):** Loro-Integration, Multi-Device, verschlüsselte Deltas.
- **P5 – UX & Reife (3–4 PM):** Blockly-Bausteine, Doku, Benchmarks, Pi-5-Optimierung, Packaging/Wheels (manylinux + macOS-arm64).
- **Summe: ca. 15–21 PM** (realistisch eher am oberen Rand, ~17–21 PM, wegen NL-Layer-Iteration und Pi-5-Tuning).

### 6. Größte Risiken & Gegenmaßnahmen
- **LLM-Genauigkeit/-Speed auf Pi 5:** Kleine Modelle machen mehr IR-Fehler und sind langsam. → Enge IR + strikter Validator + regelbasierter Parser als Fallback; LLM nur für die „letzte Meile" der Formulierung. Blockly-Modus als LLM-freie Alternative.
- **Scope-Explosion / Konkurrenz zu Lance:** Lance kann Storage + Hybrid-Search bereits selbst. → Auf Lance aufsetzen statt neu bauen; Eigenentwicklung strikt auf NL-Layer + Sandbox + Fusion + CRDT + UX fokussieren.
- **Vektor-Recall vs. Speicher auf Pi 5:** IVF-PQ statt HNSW; Produktquantisierung; kleinere Batch-Größen.
- **Wartungslast Einzelentwickler:** Konservative Crate-Auswahl (auditierte, populäre Crates wie `aes-gcm`, `roaring`, `tantivy`); klare Modul-Grenzen; abi3-Wheels reduzieren Build-Matrix.
- **Embedding-Qualität multilingual:** E5-small ist ein Kompromiss (kleinere Modelle zeigen laut mE5-Paper geringere Genauigkeit als base/large). → Upgrade-Pfad auf base/large auf stärkerer Hardware anbieten.
- **Lance-Reife:** Lance ist in aktiver Entwicklung; Format-/API-Änderungen möglich. → Version pinnen, Storage-Abstraktion kapseln.

### 7. USP-Abgrenzung
- **vs. DuckDB + LLM-Wrapper:** DuckDB ist analytisch stark, aber Vektor-/Semantik ist Add-on, kein embedded HNSW-Erststurm; ein reiner LLM-Wrapper generiert typischerweise rohes SQL (Sicherheits-/P2SQL-Risiko) statt einer validierten, sandboxed IR. SemanticDB: Vektor-first + Sandbox-IR + kindgerechte UX + Verschlüsselung.
- **vs. LanceDB:** LanceDB ist embedded und stark bei Vektor/Multimodal (SQLite-artiges Profil, ~4 MB idle / ~150 MB bei Suche laut Community-Benchmark), hat aber keinen natürlichsprachlichen, kindgerechten, sandboxed Query-Layer und keinen CRDT-Sync. SemanticDB ergänzt genau diese Schichten und nutzt Lance als Fundament.
- **vs. Qdrant:** Qdrant ist ein exzellenter, aber Server-basierter Vektor-Store (Community-Benchmark: ~400 MB RAM dauerhaft), nicht embedded-first. SemanticDB ist library-embedded (SQLite-artig) und local-first.

### 8. Messbare Erfolgs-KPIs
- **Technisch:** Recall@10 ≥ 0,90 auf Referenzkorpus; p95-Query-Latenz < 100 ms (M4) / < 500 ms (Pi 5, ohne LLM); IR-Validierungs-Fehlerrate < 1 %; NL→korrekte-IR-Rate ≥ 85 % (M4, Qwen ~7B) / ≥ 70 % (Pi 5, 1–3B).
- **Ressourcen:** Idle-RAM < 50 MB; Query-RAM < 200 MB (vergleichbar mit LanceDB-Profil).
- **Community:** GitHub-Stars, Contributor-Count, PyPI-Downloads/Monat, Zeit bis zum ersten externen PR.

---

## Namensrecherche

Prüfung auf crates.io / PyPI / npm sowie Marken/Produkte. **Wichtiger Vorbehalt:** Registry-Verfügbarkeiten wurden teils per Suche, nicht durchgängig per Live-404 geprüft — vor finaler Wahl mit `cargo search`, `npm view` und PyPI-404 verifizieren; für „Fraga"/„Klaro" ggf. formale Markenrecherche.

| Name | crates.io | PyPI | npm | Marken-/Produktkollision | Schwere |
|---|---|---|---|---|---|
| **EuleDB** (`eule`) | frei | `eule` belegt (Euler-Diagramme, v0.1.4/2022, inaktiv); **`euledb` frei** | `eule` frei (`eulejs` existiert) | „Eule" = generisches dt. Wort für Owl; keine starke Produkt-Marke | **NIEDRIG–MITTEL** |
| **Fraga** | frei | frei | frei | Fraga Inteligência Automotiva (BR) — ~30 J. alte Firma, vermarktet „die kompletteste Fahrzeug-**Datenbank** Brasiliens" (VIO/Intelliauto, 560k+ Teile) → semantische DB-Kollision | **MITTEL–HOCH** |
| **KlaroDB** (`klaro`) | frei | frei; `klarodb` frei | **`klaro` belegt** | npm-Paket „klaro": „a simple consent management platform (CMP) and privacy tool … compliant with … GDPR and ePrivacy" (KIProtect, github.com/kiprotect/klaro) — etablierte OSS-Marke | **HOCH** |
| **Semba** | frei (Nachbar `sembas` existiert) | **`semba` belegt** (Bayesian Structural Equation Modelling, semopy-Umfeld) | frei (`semba-lib` existiert) | Semba = angolanischer Musik-/Tanzstil (kulturell, keine Marke) | **MITTEL** |
| **Simsala** | frei (Nachbar `simba` existiert) | frei | **wohl belegt** (Yarn-Mirror zeigt Paket + GitHub-Org `simsala`) | Pokémon #065 (Alakazam, dt. „Simsala"); „Simsalabim" | **MITTEL** |

**Bewertung je Name (Einprägsamkeit / Marketing DACH+intl. / Kindgerechtheit / Verfügbarkeit):**

- **EuleDB** — Einprägsamkeit: hoch (kurz, bildhaft: Eule = Weisheit). Marketing DACH: sehr gut; international erklärungsbedürftig, aber sympathisch; das „EU" im Namen unterstreicht Souveränität. Kindgerecht: sehr hoch (Eule als Maskottchen). Verfügbarkeit: am besten (Paket `euledb` überall frei). **Empfehlung: Top-Kandidat.**
  - GitHub-About (EN): *„Local-first, EU-sovereign embedded hybrid database you can ask in plain language."*
  - Claim (DE): „Deine Daten. Deine Fragen. Klug wie eine Eule — und bleibt bei dir."
  - Logo-Brief: Minimalistische geometrische Eule aus zwei Kreis-Augen, die zugleich an eine Datenbank-„Disc"/zwei gestapelte Zylinder erinnern; Farbwelt tiefes Nachtblau + warmes Bernstein/Gold (Augen); flat, vektorbasiert, auch als Favicon in 16 px lesbar.

- **KlaroDB** — Einprägsamkeit: hoch („klar" passt perfekt). Aber **Kollision HOCH** (Klaro! Consent-Manager, npm belegt) → Verwechslungs- und SEO-Risiko im selben „Dev/Privacy-Tools"-Umfeld. Kindgerecht: hoch. **Empfehlung: meiden oder differenzieren** (z.B. „Klaria").
  - GitHub-About (EN): *„Clear, local-first hybrid database you can ask in plain language."*
  - Claim (DE): „Frag klar. Antwort klar. KlaroDB."
  - Logo-Brief: Klares Prisma/Wassertropfen, das Licht in Datenzeilen bricht; Farbwelt klares Cyan + Weiß; sehr reduziert.

- **Fraga** — Einprägsamkeit: hoch, „frag!" selbsterklärend (DE) und international kurz. Aber **DB-Firmen-Kollision** in Brasilien (semantisch heikel, weil dort explizit als Datenbank-Produkt vermarktet). Kindgerecht: hoch (Imperativ „frag!"). **Empfehlung: gut, aber Marken-Risiko prüfen; ggf. „Fragl"/„FragDB".**
  - GitHub-About (EN): *„Ask your data in plain language — a local-first embedded hybrid database."*
  - Claim (DE): „Einfach fragen. Fraga."
  - Logo-Brief: Sprechblase, die zugleich Fragezeichen und Datenbank-Zylinder formt; Farbwelt frisches Grün + Anthrazit; freundlich-rund.

- **Semba** — Einprägsamkeit: mittel (semantic base, nicht selbsterklärend). PyPI-Exaktname belegt → Paket „sembadb". Kindgerecht: mittel. **Empfehlung: solide zweite Wahl.**
  - GitHub-About (EN): *„Semantic base: a local-first embedded database with hybrid search."*
  - Claim (DE): „Die semantische Basis für deine Daten."
  - Logo-Brief: Drei ineinandergreifende Knoten (semantischer Graph) auf Datenbank-Basislinie; Farbwelt Violett + Petrol; modern-technisch.

- **Simsala** — Einprägsamkeit: hoch (Magie, verspielt). Aber npm-Name belegt + starke Pokémon-Assoziation. Kindgerecht: sehr hoch. **Empfehlung: charmant, aber riskant; für ein ernsthaftes Dev-Tool evtl. zu verspielt.**
  - GitHub-About (EN): *„Ask, and it appears — a magical local-first hybrid database."*
  - Claim (DE): „Frag den Zauberspruch — Simsala findet's."
  - Logo-Brief: Zauberstab, dessen Funken sich in Datenpunkte/Sterne auflösen; Farbwelt Mitternachtsviolett + Magenta-Funken; verspielt, kindlich.

## Recommendations
1. **Namen: EuleDB wählen** und Paket `euledb` auf PyPI/crates.io/npm reservieren. Vor Commit live verifizieren (`cargo search euledb`, `npm view euledb`, PyPI-404). KlaroDB (Klaro!-Marke, npm belegt) und Fraga (automotive DB-Firma) wegen Kollision meiden bzw. bewusst differenzieren.
2. **Architektur: Auf Lance aufsetzen, nicht neu bauen.** Der Wettbewerbsvorteil liegt nicht im Storage-Format, sondern im NL→IR-Layer, der Sandbox, dem Fusion-Planer, CRDT-Sync und der kindgerechten UX. Storage-Layer hinter einer Abstraktion kapseln (Lance-Version pinnen).
3. **Modellstrategie staffeln:** Default-Embedding `multilingual-e5-small` überall; LLM auf M4 = Qwen 4–8B (MLX), auf Pi 5 = 1–3B (llama.cpp Q4_K_M) + regelbasierter Fallback + Blockly als LLM-freie Alternative.
4. **Release-Reihenfolge P0→P5:** Nach P2 (Semantik + RRF-Fusion) einen ersten öffentlichen Release veröffentlichen, um Community-Feedback vor dem aufwändigen NL-Layer (P3) einzusammeln. Schwellenwert zum Weitermachen mit P3: ≥ 50 GitHub-Stars oder ≥ 3 externe Interessenten mit konkretem Use-Case.
5. **KPIs von Beginn an messen** (Recall@10, IR-Fehlerrate, RAM, tok/s auf Pi 5) und reproduzierbar im Repo veröffentlichen — das differenziert glaubwürdig von den Marketing-Claims der Konkurrenz. Wenn die NL→korrekte-IR-Rate auf dem Pi 5 unter 60 % fällt, den LLM-Pfad dort deaktivieren und auf Blockly/Regel-Parser als Default umstellen.

## Caveats
- Registry-Verfügbarkeiten wurden teils per Suche, nicht durchgängig per Live-404 geprüft — vor finaler Namenswahl verifizieren; formale Markenrecherche für „Fraga"/„Klaro" empfohlen.
- Token/Sekunde-Angaben für den Pi 5 stammen aus Community-Benchmarks (2025/26) und variieren stark je nach Quantisierung, Kühlung und Kontextlänge; es gibt keinen unterstützten GPU-Backend-Pfad für den VideoCore-VII des Pi 5 in llama.cpp — Inferenz läuft rein auf CPU.
- Die PM-Schätzungen sind Erfahrungswerte für einen erfahrenen Einzelentwickler und hängen stark vom Reifegrad der gewählten Crates und vom UX-Umfang ab.
- Lance ist in aktiver Entwicklung; Format-/API-Änderungen sind möglich (Version pinnen, Storage-Layer abstrahieren).
- Die „100x/2000x schneller als Parquet"-Angaben sind vom Lance-Projekt/LanceDB selbst publiziert und beziehen sich auf Random-Access-Mikrobenchmarks — für die eigene Workload eigenständig verifizieren.