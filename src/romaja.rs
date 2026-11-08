//! Korean as it sounds, from ccez-tube's Kotlin (itself ccez-lang's
//! `src/core/readings/romaja.ts`): the Revised Romanization's sound
//! changes across syllables (liaison 한국어 hangugeo, nasalization 합니다
//! hamnida, liquids 신라 silla, ㅎ 좋다 jota, palatalization 같이 gachi)
//! plus tensing after a stop (먹고 먹꼬 meokko), which the standard leaves
//! unwritten but a learner hears. Tensing that depends on meaning (the
//! future -ㄹ 수, 발달) can't be read off the letters and is left out.

const INITIALS: [&str; 19] = ["ㄱ", "ㄲ", "ㄴ", "ㄷ", "ㄸ", "ㄹ", "ㅁ", "ㅂ", "ㅃ", "ㅅ", "ㅆ", "ㅇ", "ㅈ", "ㅉ", "ㅊ", "ㅋ", "ㅌ", "ㅍ", "ㅎ"];
const VOWELS: [&str; 21] = ["a", "ae", "ya", "yae", "eo", "e", "yeo", "ye", "o", "wa", "wae", "oe", "yo", "u", "wo", "we", "wi", "yu", "eu", "ui", "i"];
const FINALS: [&str; 28] = [
    "", "ㄱ", "ㄲ", "ㄳ", "ㄴ", "ㄵ", "ㄶ", "ㄷ", "ㄹ", "ㄺ", "ㄻ", "ㄼ", "ㄽ", "ㄾ", "ㄿ", "ㅀ", "ㅁ", "ㅂ", "ㅄ", "ㅅ", "ㅆ", "ㅇ", "ㅈ", "ㅊ", "ㅋ", "ㅌ", "ㅍ", "ㅎ",
];
const VOWEL_I: usize = 20;

fn initial_roman(l: &str) -> &'static str {
    match l {
        "ㄱ" => "g",
        "ㄲ" => "kk",
        "ㄴ" => "n",
        "ㄷ" => "d",
        "ㄸ" => "tt",
        "ㄹ" => "r",
        "ㅁ" => "m",
        "ㅂ" => "b",
        "ㅃ" => "pp",
        "ㅅ" => "s",
        "ㅆ" => "ss",
        "ㅈ" => "j",
        "ㅉ" => "jj",
        "ㅊ" => "ch",
        "ㅋ" => "k",
        "ㅌ" => "t",
        "ㅍ" => "p",
        "ㅎ" => "h",
        _ => "",
    }
}

/// How a final sounds at the end of a syllable (the seven representative codas).
fn coda(t: &str) -> &'static str {
    match t {
        "ㄱ" | "ㄲ" | "ㄳ" | "ㄺ" | "ㅋ" => "k",
        "ㄴ" | "ㄵ" | "ㄶ" => "n",
        "ㄷ" | "ㅅ" | "ㅆ" | "ㅈ" | "ㅊ" | "ㅌ" | "ㅎ" => "t",
        "ㄹ" | "ㄼ" | "ㄽ" | "ㄾ" | "ㅀ" => "l",
        "ㅁ" | "ㄻ" => "m",
        "ㅂ" | "ㅍ" | "ㅄ" | "ㄿ" => "p",
        "ㅇ" => "ng",
        _ => "",
    }
}

/// Double finals that split when a vowel follows: what stays, what moves on.
fn split(t: &str) -> Option<(&'static str, &'static str)> {
    Some(match t {
        "ㄳ" => ("ㄱ", "ㅆ"),
        "ㄵ" => ("ㄴ", "ㅈ"),
        "ㄶ" => ("", "ㄴ"),
        "ㄺ" => ("ㄹ", "ㄱ"),
        "ㄻ" => ("ㄹ", "ㅁ"),
        "ㄼ" => ("ㄹ", "ㅂ"),
        "ㄽ" => ("ㄹ", "ㅆ"),
        "ㄾ" => ("ㄹ", "ㅌ"),
        "ㄿ" => ("ㄹ", "ㅍ"),
        "ㅀ" => ("", "ㄹ"),
        "ㅄ" => ("ㅂ", "ㅆ"),
        _ => return None,
    })
}

fn aspirate(l: &str) -> Option<&'static str> {
    Some(match l {
        "ㄱ" => "ㅋ",
        "ㄷ" => "ㅌ",
        "ㅂ" => "ㅍ",
        "ㅈ" => "ㅊ",
        "ㅅ" => "ㅌ",
        _ => return None,
    })
}

/// Plain initials and their tense counterparts.
fn tense_of(l: &str) -> Option<&'static str> {
    Some(match l {
        "ㄱ" => "ㄲ",
        "ㄷ" => "ㄸ",
        "ㅂ" => "ㅃ",
        "ㅅ" => "ㅆ",
        "ㅈ" => "ㅉ",
        _ => return None,
    })
}

fn tensed(l: &str) -> bool {
    matches!(l, "ㄲ" | "ㄸ" | "ㅃ" | "ㅆ" | "ㅉ")
}

/// The final a coda sounds as, as a letter.
fn sounded_final(c: &str) -> &'static str {
    match c {
        "k" => "ㄱ",
        "n" => "ㄴ",
        "t" => "ㄷ",
        "l" => "ㄹ",
        "m" => "ㅁ",
        "p" => "ㅂ",
        "ng" => "ㅇ",
        _ => "",
    }
}

/// Verb stems ending in these tense the ending's consonant (앉다 안따, 넓게 널께).
fn stem_tensing(t: &str) -> bool {
    matches!(t, "ㄵ" | "ㄼ" | "ㄾ")
}

#[derive(Clone)]
struct Syllable {
    l: &'static str,
    v: usize,
    t: &'static str,
}

fn decompose(ch: char) -> Option<Syllable> {
    let code = (ch as u32).checked_sub(0xac00)? as usize;
    if code > 11171 {
        return None;
    }
    Some(Syllable { l: INITIALS[code / 588], v: (code % 588) / 28, t: FINALS[code % 28] })
}

fn compose(s: &Syllable) -> char {
    let l = INITIALS.iter().position(|x| *x == s.l).unwrap_or(11);
    let t = FINALS.iter().position(|x| *x == s.t).unwrap_or(0);
    char::from_u32(0xac00 + ((l * 21 + s.v) * 28 + t) as u32).unwrap_or('?')
}

/// The sound changes between one syllable's final and the next one's initial.
fn boundary(a: &mut Syllable, b: &mut Syllable) {
    let t = a.t;
    let l = b.l;
    if t.is_empty() {
        return;
    }
    if l == "ㅇ" {
        if t == "ㅇ" {
            return;
        }
        if t == "ㅎ" {
            a.t = "";
            return;
        }
        if let Some((stays, moves)) = split(t) {
            a.t = stays;
            if !moves.is_empty() {
                b.l = moves;
            }
            return;
        }
        // Palatalization: 굳이 guji, 같이 gachi.
        if b.v == VOWEL_I && (t == "ㄷ" || t == "ㅌ") {
            a.t = "";
            b.l = if t == "ㄷ" { "ㅈ" } else { "ㅊ" };
            return;
        }
        a.t = "";
        b.l = t;
        return;
    }
    if l == "ㅎ" {
        // 축하 chuka, 밝히다 balkida, 못하다 motada.
        if t == "ㄺ" || t == "ㄼ" {
            a.t = "ㄹ";
            b.l = if t == "ㄺ" { "ㅋ" } else { "ㅍ" };
            return;
        }
        if let Some(asp) = aspirate(t) {
            a.t = "";
            b.l = asp;
        }
        return;
    }
    if t == "ㅎ" || t == "ㄶ" || t == "ㅀ" {
        let rest = match t {
            "ㅎ" => "",
            "ㄶ" => "ㄴ",
            _ => "ㄹ",
        };
        let asp = if matches!(l, "ㄱ" | "ㄷ" | "ㅈ") { aspirate(l) } else { None };
        if let Some(asp) = asp {
            // 좋다 jota, 많고 manko.
            a.t = rest;
            b.l = asp;
        } else if l == "ㅅ" {
            a.t = rest;
            b.l = "ㅆ";
        } else if l == "ㄴ" && t == "ㅎ" {
            a.t = "ㄴ";
        } else {
            a.t = rest;
        }
        if t == "ㅎ" || l != "ㄴ" {
            return;
        }
    }
    let c = coda(a.t);
    if l == "ㄴ" || l == "ㅁ" {
        // Nasalization: 국민 gungmin, 합니다 hamnida; ㄹ+ㄴ → ll (설날 seollal).
        match c {
            "k" => a.t = "ㅇ",
            "t" => a.t = "ㄴ",
            "p" => a.t = "ㅁ",
            "l" if l == "ㄴ" => {
                a.t = "ㄹ";
                b.l = "ㄹ";
            }
            _ => {}
        }
        return;
    }
    if l == "ㄹ" {
        // 신라 silla, 종로 jongno, 독립 dongnip, 협력 hyeomnyeok.
        if c == "n" || c == "l" {
            a.t = "ㄹ";
            return;
        }
        b.l = "ㄴ";
        match c {
            "k" => a.t = "ㅇ",
            "p" => a.t = "ㅁ",
            "t" => a.t = "ㄴ",
            _ => {}
        }
    }
}

/// Syllables as Latin letters. Spelled (`sounded` false) is the Revised
/// Romanization; sounded, a tense initial swallows the stop before it
/// (먹꼬 meokko, 읻따 itta), as it is heard.
fn roman(run: &[Syllable], sounded: bool) -> String {
    let mut out = String::new();
    for (i, s) in run.iter().enumerate() {
        let prev = i.checked_sub(1).map(|p| &run[p]);
        // ㄹ after ㄹ is written l (silla), r before a vowel elsewhere.
        out.push_str(if s.l == "ㄹ" && prev.is_some_and(|p| coda(p.t) == "l") { "l" } else { initial_roman(s.l) });
        out.push_str(VOWELS[s.v]);
        let mut c = coda(s.t);
        if sounded && let Some(next) = run.get(i + 1) && tensed(next.l) {
            let ni = initial_roman(next.l);
            // The stop is held, not released: t before any tense consonant, k before kk, p before pp.
            if c == "t" || (!c.is_empty() && ni == format!("{c}{c}")) {
                c = "";
            }
        }
        out.push_str(c);
    }
    out
}

fn sound_run(mut run: Vec<Syllable>) -> Vec<Syllable> {
    for i in 0..run.len().saturating_sub(1) {
        let written = run[i].t;
        let (a, b) = run.split_at_mut(i + 1);
        let (a, b) = (&mut a[i], &mut b[0]);
        boundary(a, b);
        let Some(tense) = tense_of(b.l) else { continue };
        let c = coda(a.t);
        if c == "k" || c == "t" || c == "p" || (stem_tensing(written) && b.l != "ㅂ") {
            b.l = tense;
        }
    }
    for s in &mut run {
        s.t = sounded_final(coda(s.t));
    }
    run
}

/// Each run of Hangul syllables through `f`; anything else passes through.
fn each_run(text: &str, f: impl Fn(Vec<Syllable>) -> String) -> String {
    let mut out = String::new();
    let mut run: Vec<Syllable> = Vec::new();
    for ch in text.chars() {
        if let Some(s) = decompose(ch) {
            run.push(s);
            continue;
        }
        if !run.is_empty() {
            out.push_str(&f(std::mem::take(&mut run)));
        }
        out.push(ch);
    }
    if !run.is_empty() {
        out.push_str(&f(run));
    }
    out
}

/// The Revised Romanization: 한국어 → hangugeo, 먹고 → meokgo.
pub fn romanize(text: &str) -> String {
    each_run(text, |mut run| {
        for i in 0..run.len().saturating_sub(1) {
            let (a, b) = run.split_at_mut(i + 1);
            boundary(&mut a[i], &mut b[0]);
        }
        roman(&run, false)
    })
}

/// How it sounds, in Hangul: 먹고 → 먹꼬, 한국어 → 한구거, 같이 → 가치.
pub fn sounds_like(text: &str) -> String {
    each_run(text, |run| sound_run(run).iter().map(compose).collect())
}

/// How it sounds, romanized as said: 먹고 → meokko, 있다 → itta.
pub fn sounds_like_roman(text: &str) -> String {
    each_run(text, |run| roman(&sound_run(run), true))
}

pub fn has_hangul(text: &str) -> bool {
    text.chars().any(|c| ('가'..='힣').contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revised_romanization_with_sound_changes() {
        assert_eq!(romanize("한국어"), "hangugeo");
        assert_eq!(romanize("합니다"), "hamnida");
        assert_eq!(romanize("신라"), "silla");
        assert_eq!(romanize("좋다"), "jota");
        assert_eq!(romanize("같이"), "gachi");
        assert_eq!(romanize("국민"), "gungmin");
        assert_eq!(romanize("먹고"), "meokgo");
        assert_eq!(romanize("밥을 먹어요."), "babeul meogeoyo.");
    }

    #[test]
    fn as_it_sounds() {
        assert_eq!(sounds_like("먹고"), "먹꼬");
        assert_eq!(sounds_like("한국어"), "한구거");
        assert_eq!(sounds_like("같이"), "가치");
        assert_eq!(sounds_like_roman("먹고"), "meokko");
        assert_eq!(sounds_like_roman("있다"), "itta");
    }
}
