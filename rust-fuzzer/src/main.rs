use std::collections::BTreeSet;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BATCH_SIZE: usize = 10;

#[derive(Default)]
struct Statistics {
    fuzz_cases: AtomicUsize,
    crashes: AtomicUsize,
}

struct Rng(u64);

impl Rng {
    fn new() -> Self {
        Rng(0x6fa1bed31f2e77dd ^ unsafe { std::arch::x86_64::_rdtsc() })
    }

    #[inline]
    fn rand(&mut self) -> usize {
        let val = self.0;
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 43;
        val as usize
    }
}

fn fuzz<P: AsRef<Path>>(filename: P, input: &[u8]) -> io::Result<ExitStatus> {
    std::fs::write(filename.as_ref(), input)?;

    let runner = Command::new("./objdump")
        .args(&["-x", filename.as_ref().to_str().unwrap()])
        .output()?;

    Ok(runner.status)
}

fn worker(
    thread_id: usize,
    statistics: Arc<Statistics>,
    corpus: Arc<Vec<Vec<u8>>>,
) -> io::Result<()> {
    let mut rng = Rng::new();
    let filename = format!("tmpinput{}", thread_id);
    let mut fuzz_input = Vec::new();

    loop {
        for _ in 0..BATCH_SIZE {
            let sel = rng.rand() % corpus.len();
            fuzz_input.clear();
            fuzz_input.extend_from_slice(&corpus[sel]);

            for _ in 0..(rng.rand() % 8) + 1 {
                let sel = rng.rand() % fuzz_input.len();
                fuzz_input[sel] = rng.rand() as u8;
            }

            let exit = fuzz(&filename, &fuzz_input)?;
            if let Some(11) = exit.signal() {
                statistics.crashes.fetch_add(1, Ordering::SeqCst);
            }
        }

        statistics.fuzz_cases.fetch_add(1, Ordering::SeqCst);
    }
}

fn main() -> io::Result<()> {
    let mut corpus = BTreeSet::new();
    for filename in std::fs::read_dir("./corpus")? {
        let filename = filename?.path();
        corpus.insert(std::fs::read(filename)?);
    }
    let corpus: Arc<Vec<Vec<u8>>> = Arc::new(corpus.into_iter().collect());

    println!("Loaded {} files into corpus", corpus.len());

    let stats = Arc::new(Statistics::default());

    for thread_id in 0..1 {
        let stats = stats.clone();
        let corpus = corpus.clone();
        std::thread::spawn(move || worker(thread_id, stats, corpus));
    }

    let start = Instant::now();

    loop {
        std::thread::sleep(Duration::from_millis(1000));

        let elapsed = start.elapsed().as_secs_f64();
        let cases = stats.fuzz_cases.load(Ordering::SeqCst);
        let crashes = stats.crashes.load(Ordering::SeqCst);
        let fcps = cases as f64 / elapsed;
        println!(
            "[{:10.6}] cases {:10} | fcps {:10.2} | crashes {:10}",
            elapsed, cases, fcps, crashes
        );
    }
}
