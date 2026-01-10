### **Interni Vodič za Pisanje Rust Koda**

**Status:** Aktivan
**Svrha:** Ovaj dokument definira standarde i najbolje prakse za pisanje Rust koda unutar naše tvrtke. Cilj je osigurati da je sav kod konzistentan, čitljiv, idiomatski i robustan. Pridržavanje ovih pravila je ključno za dugoročnu održivost naših projekata.

---

## **1. Alati za Kvalitetu Koda**

Naš razvojni proces se oslanja na dva temeljna alata iz Rust ekosustava kako bi se automatizirala i standardizirala kvaliteta koda.

### **1.1. Formatiranje: `rustfmt`**

Sve rasprave o stilu (razmaci, prijelomi linija, uvlačenje) su eliminirane korištenjem službenog Rust formatera.

*   **Alat:** `rustfmt`
*   **Konfiguracija:** Sva pravila su centralizirana u `rustfmt.toml` datoteci u korijenu projekta.
*   **Pravilo:** Prije svakog `git commit`, developer je dužan pokrenuti `cargo fmt --all` kako bi osigurao da je kod formatiran prema standardu. CI pipeline će provjeravati formatiranje i odbaciti promjene koje nisu u skladu s pravilima.

### **1.2. Linting (Statička Analiza): `clippy`**

Clippy je naš "virtualni senior developer" koji pronalazi česte greške, neidiomatski kod, potencijalne probleme s performansama i predlaže poboljšanja.

*   **Alat:** `clippy`
*   **Konfiguracija:** Pravila su centralno definirana u `Cargo.toml` datoteci radnog prostora (`[workspace.lints.clippy]`).
*   **Pravilo:** Naš CI pipeline pokreće `cargo clippy` sa zastavicom koja sva upozorenja (`warnings`) tretira kao greške (`errors`). Developer je dužan pokrenuti `cargo clippy --workspace` lokalno kako bi ispravio sve prijavljene probleme prije slanja koda na reviziju.

---

## **2. Idiomatsko Pisanje Koda**

Ovo su ključni obrasci na koje `clippy` često upozorava, a njihovo usvajanje čini kod čišćim, sigurnijim i lakšim za čitanje.

### **2.1. Iteratori umjesto C-style Petlji**

Rustov sustav iteratora je moćan i siguran. Ručno upravljanje indeksima je sklono greškama i smatra se neidiomatskim.

*   **Problem:** Korištenje `for i in 0..vec.len() { vec[i] }`.
*   **Rješenje:** Koristite iteratore direktno.

*   **Loš Primjer (izbjegavati):**
    ```rust
    let items = vec!["a", "b", "c"];
    for i in 0..items.len() {
        println!("Item: {}", items[i]);
    }
    ```

*   **Dobar Primjer:**
    ```rust
    let items = vec!["a", "b", "c"];

    // Ako vam ne treba indeks:
    for item in &items {
        println!("Item: {}", item);
    }

    // Ako vam trebaju i indeks i vrijednost:
    for (i, item) in items.iter().enumerate() {
        println!("Item na indeksu {}: {}", i, item);
    }
    ```

### **2.2. Elegantno Rukovanje Greškama unutar Petlji**

Kada iterator vraća `Result` ili `Option`, izbjegavajte ručnu provjeru s `if let`.

*   **Problem:** `for result in iterator { if let Ok(value) = result { ... } }`.
*   **Rješenje:** Koristite metodu `.flatten()`, koja automatski filtrira sve `Err` ili `None` varijante.

*   **Loš Primjer (izbjegavati):**
    ```rust
    let results = vec![Ok(1), Err("greška"), Ok(3)];
    for result in results {
        if let Ok(value) = result {
            println!("Vrijednost: {}", value);
        }
    }
    ```

*   **Dobar Primjer:**
    ```rust
    let results = vec![Ok(1), Err("greška"), Ok(3)];
    for value in results.into_iter().flatten() {
        println!("Vrijednost: {}", value);
    }
    ```

### **2.3. Funkcionalne Metode za `Option` i `Result`**

Izbjegavajte ručno pisanje `match` ili `if let` blokova kada postoji ugrađena funkcionalna metoda koja radi istu stvar.

*   **Problem:** Ručna implementacija `map` logike.
*   **Rješenje:** Koristite `.map()`, `.and_then()`, `.is_some_and()`, `.unwrap_or()` itd.

*   **Loš Primjer (izbjegavati):**
    ```rust
    let maybe_number: Option<i32> = Some(5);
    let maybe_incremented = if let Some(n) = maybe_number {
        Some(n + 1)
    } else {
        None
    };
    ```

*   **Dobar Primjer:**
    ```rust
    let maybe_number: Option<i32> = Some(5);
    let maybe_incremented = maybe_number.map(|n| n + 1);
    ```

### **2.4. Dizajn Struktura i `Default` Trait**

Standardna je konvencija da strukture koje se mogu kreirati bez argumenata implementiraju `Default` trait.

*   **Problem:** Imati `fn new() -> Self` bez implementacije `Default`.
*   **Rješenje:** Implementirajte `Default` i neka `new()` bude samo alias za `default()`.

*   **Loš Primjer (izbjegavati):**
    ```rust
    pub struct Config;
    impl Config {
        pub fn new() -> Self { Config }
    }
    ```

*   **Dobar Primjer:**
    ```rust
    pub struct Config;

    impl Default for Config {
        fn default() -> Self {
            Config
        }
    }

    impl Config {
        pub fn new() -> Self {
            Self::default()
        }
    }
    ```

---

## **3. Rukovanje Greškama: Nema Panike!**

Panika (`panic!`) uzrokuje pad programa i treba se koristiti samo u situacijama gdje je oporavak nemoguć (npr. u testovima za provjeru neispravnog stanja).

*   **Pravilo:** Korištenje `.unwrap()` i `.expect()` je **strogo zabranjeno** u produkcijskom kodu (biblioteke i aplikacije).
*   **Obrazloženje:** Robusni softver mora rukovati greškama, a ne se rušiti.
*   **Ispravan način:** Koristite `?` operator za propagaciju grešaka prema gore u pozivnom stogu i `match` ili `if let Err(...)` za rukovanje greškama na mjestu gdje se mogu obraditi.

*   **Loš Primjer (Zabranjeno):**
    ```rust
    let content = std::fs::read_to_string("config.toml").unwrap();
    ```

*   **Dobar Primjer (unutar funkcije koja vraća `Result`):**
    ```rust
    fn read_config() -> Result<String, std::io::Error> {
        let content = std::fs::read_to_string("config.toml")?;
        Ok(content)
    }
