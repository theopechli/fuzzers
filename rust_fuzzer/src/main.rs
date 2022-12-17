use clap::Parser;
use sha2::{Digest, Sha256};
use std::fs;
use std::{
    collections::BTreeSet,
    fs::File,
    io,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    sync::atomic::{AtomicUsize, Ordering},
    sync::Arc,
    time::{Duration, Instant},
};

const BATCH_SIZE: usize = 10;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long, value_name = "FUZZ TARGET")]
    binary: PathBuf,

    #[arg(short, long)]
    flag: String,

    #[arg(short, long)]
    corpus: PathBuf,

    #[arg(short, long)]
    output: PathBuf,

    #[arg(short, long)]
    threads: usize,
}

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

fn fuzz<P: AsRef<Path>>(
    binary: &PathBuf,
    flag: &String,
    filename: P,
    input: &[u8],
) -> io::Result<ExitStatus> {
    std::fs::write(filename.as_ref(), input)?;

    let runner = Command::new(binary)
        .arg(flag)
        .arg(filename.as_ref().to_str().unwrap())
        .output()?;

    Ok(runner.status)
}

fn worker(
    thread_id: usize,
    statistics: Arc<Statistics>,
    binary: PathBuf,
    flag: String,
    corpus: Arc<Vec<Vec<u8>>>,
    output_dir: PathBuf,
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

            let exit = fuzz(&binary, &flag, &filename, &fuzz_input)?;
            if let Some(11) = exit.signal() {
                let mut hasher = Sha256::new();
                let input = fuzz_input.clone();
                hasher.update(&input);
                let result = hasher.finalize();
                std::fs::write(
                    format!("{}/crash_{:#04x}", output_dir.display(), result),
                    &input,
                )?;
                statistics.crashes.fetch_add(1, Ordering::SeqCst);
            }
        }

        statistics.fuzz_cases.fetch_add(1, Ordering::SeqCst);
    }
}

fn main() -> io::Result<()> {
    let args = Cli::parse();

    let binary = args.binary;
    println!("Fuzz target is: '{}'", binary.display());
    {
        File::open(&binary)
            .unwrap_or_else(|err| panic!("Could not open file '{}': {}", binary.display(), err));
    }

    let flag = &["--", &args.flag].join("");
    println!("Flag is: '{}'", flag);

    let corpus_dir = args.corpus;
    println!("Corpus directory is: '{}'", corpus_dir.display());
    {
        File::open(&corpus_dir).unwrap_or_else(|err| {
            panic!(
                "Could not open corpus directory '{}': {:?}",
                corpus_dir.display(),
                err
            )
        });
    }

    let output_dir = args.output;
    println!("Output directory is: '{}'", output_dir.display());
    match fs::create_dir_all(&output_dir) {
        Ok(()) => println!(
            "Created missing output directory: '{}'",
            output_dir.display()
        ),
        Err(e) => panic!(
            "Could not create missing output directory '{}': {:?}",
            output_dir.display(),
            e
        ),
    }

    let thread_count = args.threads;
    println!("Thread count is: {}", thread_count);

    let mut corpus = BTreeSet::new();
    for filename in std::fs::read_dir(corpus_dir)? {
        let filename = filename?.path();
        corpus.insert(std::fs::read(filename)?);
    }
    let corpus: Arc<Vec<Vec<u8>>> = Arc::new(corpus.into_iter().collect());

    println!("Loaded {} files into corpus", corpus.len());

    let stats = Arc::new(Statistics::default());

    for thread_id in 0..thread_count {
        let stats = stats.clone();
        let binary = binary.clone();
        let flag = flag.clone();
        let corpus = corpus.clone();
        let output_dir = output_dir.clone();
        std::thread::spawn(move || worker(thread_id, stats, binary, flag, corpus, output_dir));
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
