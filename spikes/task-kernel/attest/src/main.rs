//! The operator's program: `ask` and `verify`. A throwaway spike, not part of the workspace or
//! the trusted core.
//!
//! `ask` sends one question to the model provider over a TLS session that the customer's notary
//! takes part in without seeing the plaintext, and gets the notary's signature over a commitment
//! to the bytes the provider's server sent. It then writes a presentation: the whole exchange with
//! the credential's bytes hidden. `verify` checks a presentation with no network and no credential.
//!
//! The notary is a program of its own, reached over a pinned, encrypted link. This program holds
//! only its own transport key and the notary's two public keys.
//!
//! Every request names the run it was made for and its position in that run, inside the attested
//! bytes. The notary adds the run it was told and how many attestations it had signed for that
//! run, under its one signature.
//!
//! The credential is read here, goes to the provider and nowhere else: not into the presentation,
//! a message, or an error.

use anyhow::{Context, Result, anyhow, bail, ensure};
use attest::{
    Answer, CREDENTIAL_LENGTH, DEADLINE, END, MAX_QUESTION, MAX_RECV, MAX_SENT, Trust, answer, approved, binding, find,
    link::{self, Identity, LinkFailure},
    pinned_credential, read_run_sequence, request, written_head,
};
use futures::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    future::IntoFuture,
    io::Read,
    ops::Range,
    path::Path,
    pin::Pin,
    process::ExitCode,
    sync::{Arc, atomic::{AtomicBool, Ordering}},
    task::{Context as TaskContext, Poll},
    time::Instant,
};
use tlsn::{
    Session,
    attestation::{
        Attestation, CryptoProvider,
        presentation::{Presentation, PresentationOutput},
        request::{Request as AttestationRequest, RequestConfig},
    },
    config::{prove::ProveConfig, prover::ProverConfig, tls::TlsClientConfig, tls_commit::mpc::MpcTlsConfig},
    connection::HandshakeData,
    prover::ProverOutput,
    transcript::TranscriptCommitConfig,
    webpki::ServerCertVerifier,
};
use tokio_util::compat::TokioAsyncReadCompatExt;

/// Why ask refused, in the closed set of words the monitor maps to its own reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Word {
    Unreachable,
    Deadline,
    NotaryKey,
    Release,
    Provider,
}

impl std::fmt::Display for Word {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Word::Unreachable => "unreachable",
            Word::Deadline => "deadline",
            Word::NotaryKey => "notary-key",
            Word::Release => "release",
            Word::Provider => "provider",
        })
    }
}

impl std::error::Error for Word {}

/// Where the notary is and which keys it must hold, from the environment a monitor passes on.
struct NotaryLink {
    address: String,
    transport: String,
    signer: String,
    me: Identity,
}

impl NotaryLink {
    fn read() -> Result<Self> {
        let set = |name: &str| env::var(name).ok().filter(|value| !value.is_empty()).with_context(|| format!("set {name}"));
        let (address, transport, signer) = (set("HYALOS_NOTARY_ADDRESS")?, set("HYALOS_NOTARY_TRANSPORT_PUBLIC")?, set("HYALOS_NOTARY_SIGNER_PUBLIC")?);
        let me = Identity::load(Path::new(&set("HYALOS_OPERATOR_TRANSPORT_KEY")?)).map_err(|_| anyhow!("cannot use the operator's transport key"))?;
        Ok(Self { address, transport, signer, me })
    }
}

/// The link's stream, remembering whether it ended or failed, so a failed session can be told
/// apart from a failed provider without reading any error's text.
struct Watched<S> {
    inner: S,
    broke: Arc<AtomicBool>,
}

impl<S: AsyncRead + Unpin> AsyncRead for Watched<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &mut [u8]) -> Poll<std::io::Result<usize>> {
        let polled = Pin::new(&mut self.inner).poll_read(cx, buf);
        if matches!(&polled, Poll::Ready(Err(_))) || matches!(&polled, Poll::Ready(Ok(0)) if !buf.is_empty()) {
            self.broke.store(true, Ordering::SeqCst);
        }
        polled
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Watched<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        let polled = Pin::new(&mut self.inner).poll_write(cx, buf);
        if matches!(&polled, Poll::Ready(Err(_))) {
            self.broke.store(true, Ordering::SeqCst);
        }
        polled
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}

fn home(file: &str) -> Result<String> {
    Ok(format!("{}/.config/hyalos-spike/{file}", env::var("HOME").context("no home")?))
}

fn credential() -> Result<String> {
    let text = fs::read_to_string(home("anthropic.key")?).map_err(|_| anyhow!("cannot use the credential file"))?;
    let line = text.lines().find(|l| !l.trim().is_empty()).context("cannot use the credential file")?.trim();
    let value = line.split_once('=').map_or(line, |(_, v)| v).trim().trim_matches(['"', '\'']);
    ensure!(!value.is_empty() && value.bytes().all(|b| b.is_ascii_graphic()), "cannot use the credential file");
    Ok(value.to_string())
}

/// Asks one question through a notarized session and returns the answer with its presentation.
async fn ask(title: &str, binding: &str, run: &str, trust: &Trust, credential: &str, notary: &NotaryLink, broke: &Arc<AtomicBool>) -> Result<(Answer, Vec<u8>)> {
    let (bytes, hidden) = request(title, binding, &trust.server, credential);
    ensure!(bytes.len() <= MAX_SENT - 256, "the question is too large to attest");

    let said = |failure| match failure {
        LinkFailure::PeerKey => Word::NotaryKey,
        LinkFailure::Unreachable => Word::Unreachable,
        LinkFailure::Sequence => Word::Provider,
    };
    let mut stream = link::connect(&notary.address, &notary.me, &notary.transport).await.map_err(said)?;
    if !link::open(&mut stream, run).await.map_err(said)? {
        return Err(Word::Release.into());
    }

    let session = Session::new(Watched { inner: stream.compat(), broke: broke.clone() });
    let (driver, mut handle) = session.split();
    let driver_task = tokio::spawn(driver);
    let prover = handle
        .new_prover(ProverConfig::builder().build()?)?
        .commit(MpcTlsConfig::builder().max_sent_data(MAX_SENT).max_recv_data(MAX_RECV).build()?)
        .await?;
    let client_socket = tokio::net::TcpStream::connect(trust.address.as_str()).await?;
    client_socket.set_nodelay(true)?;
    let (mut tls, prover) = prover.connect(
        TlsClientConfig::builder().server_name(trust.name()?).root_store(trust.roots.clone()).build()?,
        client_socket.compat(),
    )?;
    let prover_task = tokio::spawn(prover.into_future());

    tls.write_all(&bytes).await?;
    tls.flush().await?;
    let mut reply = Vec::new();
    tls.read_to_end(&mut reply).await?;
    // For the test of an operator who reads a reply and leaves to ask again. It is no door: a forger
    // can change this program the same way, and the number is already spent (ADR-014).
    if env::var_os("HYALOS_TEST_LEAVE_AFTER_REPLY").is_some() {
        return Err(Word::Provider.into());
    }
    let _ = tls.close().await;
    let mut prover = prover_task.await??;

    // What the session really carried, not what this function believes it sent or read.
    let (sent, received) = (prover.transcript().sent().to_vec(), prover.transcript().received().to_vec());
    ensure!(sent == bytes, "the session did not carry the request as written");
    let line = find(&received, b"\r\n").context("the reply has no status line")?;
    let blank = find(&received, END).context("the reply has no body")?;

    // Commit to the exchange in pieces, so the credential and the reply's headers can stay hidden.
    let mut commit = TranscriptCommitConfig::builder(prover.transcript());
    commit.commit_sent(&(0..hidden.start))?;
    commit.commit_sent(&hidden)?;
    commit.commit_sent(&(hidden.end..sent.len()))?;
    commit.commit_recv(&(0..line))?;
    commit.commit_recv(&(line..blank))?;
    commit.commit_recv(&(blank..received.len()))?;
    let mut request_config = RequestConfig::builder();
    request_config.transcript_commit(commit.build()?);
    let request_config = request_config.build()?;
    let mut prove = ProveConfig::builder(prover.transcript());
    // The notary compares this name, verified against its own roots, with the one it approves.
    prove.server_identity();
    if let Some(config) = request_config.transcript_commit() {
        prove.transcript_commit(config.clone());
    }
    let ProverOutput { transcript_commitments, transcript_secrets, .. } = prover.prove(&prove.build()?).await?;
    let (transcript, tls_transcript) = (prover.transcript().clone(), prover.tls_transcript().clone());
    prover.close().await?;

    let mut builder = AttestationRequest::builder(&request_config);
    builder
        .server_name(trust.name()?)
        .handshake_data(HandshakeData {
            certs: tls_transcript.server_cert_chain().context("no certificate chain")?.to_vec(),
            sig: tls_transcript.server_signature().context("no server signature")?.clone(),
            binding: tls_transcript.certificate_binding().clone(),
        })
        .transcript(transcript)
        .transcript_commitments(transcript_secrets, transcript_commitments);
    let provider = CryptoProvider::default();
    let (request, secrets) = builder.build(&provider)?;

    handle.close();
    let mut socket = driver_task.await??;
    socket.write_all(&bincode::serialize(&request)?).await?;
    socket.close().await?;
    let attestation_bytes = link::read_bounded(&mut socket).await?;
    // A notary that refused the session closes with no attestation.
    if attestation_bytes.is_empty() {
        return Err(Word::Provider.into());
    }
    let attestation: Attestation = bincode::deserialize(&attestation_bytes).map_err(|_| Word::Provider)?;
    request.validate(&attestation, &provider).map_err(|_| Word::Provider)?;

    // The secrets hold the credential. They are used once, here, and never stored.
    let mut proof = secrets.transcript_proof_builder();
    proof.reveal_sent(&(0..hidden.start))?;
    proof.reveal_sent(&(hidden.end..sent.len()))?;
    proof.reveal_recv(&(0..line))?;
    proof.reveal_recv(&(blank..received.len()))?;
    let mut presentation = attestation.presentation_builder(&provider);
    presentation.identity_proof(secrets.identity_proof()).transcript_proof(proof.build()?);
    let presentation: Presentation = presentation.build()?;
    if hex::encode(&presentation.verifying_key().data) != notary.signer {
        return Err(Word::NotaryKey.into());
    }
    // Read only now, so a reply that is no label still gets its attestation.
    let answer = answer(&received[..line], &received[blank..]).map_err(|_| Word::Provider)?;
    Ok((answer, bincode::serialize(&presentation)?))
}

/// Checks a presentation offline. On success, what a stranger may now believe, as JSON.
fn verify(bytes: &[u8], trusted_notary: &str, trust: &Trust) -> Result<Value> {
    let presentation: Presentation = bincode::deserialize(bytes).context("not a presentation")?;
    let notary = hex::encode(&presentation.verifying_key().data);
    ensure!(notary == trusted_notary, "signed by a notary that is not the trusted one");
    let mut provider = CryptoProvider::default();
    provider.cert = ServerCertVerifier::new(&trust.roots).map_err(|_| anyhow!("cannot use the trusted roots"))?;
    let PresentationOutput { server_name, connection_info, transcript, extensions, .. } =
        presentation.verify(&provider).map_err(|_| anyhow!("the presentation does not verify"))?;
    let server = server_name.context("no server identity")?.to_string();
    ensure!(server == trust.server, "the session was with another server");
    let (run, sequence) = read_run_sequence(&extensions)?;
    let transcript = transcript.context("no transcript")?;

    // Sent: everything is shown except one run of bytes, which must be the credential's value.
    let sent = transcript.sent_unsafe(transcript.len_sent()).map_err(|_| anyhow!("bad transcript"))?;
    let shown: Vec<Range<usize>> = transcript.sent_authed().clone().into_inner();
    ensure!(shown.len() == 2 && shown[0].start == 0 && shown[1].end == sent.len(), "the request is not shown as expected");
    let head_end = find(&sent, END).context("no request body")?;
    ensure!(shown[1].start == head_end, "something other than the last header's value is hidden");
    ensure!(shown[1].start - shown[0].end == CREDENTIAL_LENGTH, "the hidden bytes are not a credential's length");
    let body_bytes = &sent[head_end + END.len()..];
    let (title, intent, position) = approved(body_bytes)?;
    ensure!(written_head(&sent[..shown[0].end], &title, &binding(&intent, position)?, &trust.server), "the request's head is not the one this tool writes");

    // Received: the status line and everything from the blank line on.
    let received = transcript.received_unsafe(transcript.len_received()).map_err(|_| anyhow!("bad transcript"))?;
    let shown: Vec<Range<usize>> = transcript.received_authed().clone().into_inner();
    ensure!(shown.len() == 2 && shown[0].start == 0 && shown[1].end == received.len(), "the reply is not shown as expected");
    let answer = answer(&received[shown[0].clone()], &received[shown[1].clone()])?;

    let mut report = json!({
        "server": server,
        "time": connection_info.time,
        "notary": notary,
        "run": run,
        "sequence": sequence,
        "intent_hash": intent,
        "position": position,
        "question_sha256": hex::encode(Sha256::digest(title.as_bytes())),
        "label": answer.label,
        "model": answer.model,
        "response_id": answer.response_id,
    });
    if trust.test {
        // Not the real provider: say so in the report itself, so it cannot be read as one.
        report["trust"] = json!("test trust settings");
    }
    Ok(report)
}

/// Ask's one word for a failed call. The error's text is never shown: it could quote the exchange.
fn word_for(error: &anyhow::Error, started: Instant, broke: &AtomicBool) -> Word {
    if let Some(word) = error.downcast_ref::<Word>() {
        *word
    } else if started.elapsed() >= DEADLINE {
        Word::Deadline
    } else if broke.load(Ordering::SeqCst) {
        Word::Unreachable
    } else {
        Word::Provider
    }
}

async fn run() -> Result<ExitCode> {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("ask") => {
            let usage = "usage: ask <presentation file> <intent hash> <position>, the question on standard input";
            let out = args.get(2).context(usage)?;
            let intent = args.get(3).context(usage)?;
            let position: u64 = args.get(4).context(usage)?.parse().map_err(|_| anyhow!("the position is not a number"))?;
            let binding = binding(intent, position)?;
            let trust = Trust::read()?;
            let notary = NotaryLink::read()?;
            let mut title = String::new();
            std::io::stdin().take(MAX_QUESTION as u64 + 1).read_to_string(&mut title).context("the question is not text")?;
            ensure!(title.len() <= MAX_QUESTION, "the question is too large");
            let credential = credential()?;
            // Test settings can point anywhere, so a credential shaped like a real one never follows them.
            ensure!(!(trust.test && credential.starts_with("sk-ant-")), "refusing to send a real credential under test trust settings");
            // Refused before anything is sent: verify would refuse the presentation anyway.
            pinned_credential(&credential)?;
            // For the test of an operator who tells the notary another run. It is no door: a forger
            // can change this program the same way, and the audit refuses what comes out.
            let told = match env::var("HYALOS_TEST_TOLD_RUN") {
                Ok(run) => run,
                Err(env::VarError::NotPresent) => intent.clone(),
                Err(env::VarError::NotUnicode(_)) => bail!("the run to tell the notary is not text"),
            };
            let started = Instant::now();
            let broke = Arc::new(AtomicBool::new(false));
            let call = tokio::time::timeout(DEADLINE, ask(&title, &binding, &told, &trust, &credential, &notary, &broke)).await;
            let checked = match call {
                Err(_) => Err(Word::Deadline),
                Ok(Err(error)) => Err(word_for(&error, started, &broke)),
                // The sequence number is read back from the presentation, as an auditor would read it.
                Ok(Ok((answer, presentation))) if find(&presentation, credential.as_bytes()).is_none() => match verify(&presentation, &notary.signer, &trust) {
                    Ok(report) => Ok((answer, presentation, report["sequence"].clone())),
                    Err(_) => Err(Word::Provider),
                },
                Ok(Ok(_)) => Err(Word::Provider),
            };
            let (answer, presentation, sequence) = match checked {
                Ok(done) => done,
                Err(word) => {
                    println!("{word}");
                    return Ok(ExitCode::FAILURE);
                }
            };
            fs::write(out, &presentation).context("cannot write the presentation")?;
            let mut said = json!({"label": answer.label, "model": answer.model, "response_id": answer.response_id,
                                  "presentation_sha256": hex::encode(Sha256::digest(&presentation)), "sequence": sequence});
            if trust.test {
                said["trust"] = json!("test trust settings");
            }
            println!("{said}");
        }
        Some("verify") => {
            let bytes = fs::read(args.get(2).context("usage: verify <presentation file> <trusted notary public key>")?).context("cannot read")?;
            println!("{}", verify(&bytes, args.get(3).context("name the notary public key you trust")?, &Trust::read()?)?);
        }
        Some("link-key") => {
            // The operator's own transport key: made once, owner-only. Prints only its public half.
            let file = args.get(2).context("usage: link-key <file>")?;
            println!("{}", Identity::make_or_load(Path::new(file))?.public);
        }
        _ => bail!("usage: ask | verify | link-key"),
    }
    Ok(ExitCode::SUCCESS)
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("refused: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::BufRead, process::{Child, Command, Stdio}, time::Duration};

    /// Removes a test's directory and ends its programs however the test ends.
    struct Rig(std::path::PathBuf, Vec<Child>);
    impl Drop for Rig {
        fn drop(&mut self) {
            for child in &mut self.1 {
                let _ = child.kill();
                let _ = child.wait();
            }
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A real session against the stand-in and a notary of its own, with `credential` as the
    /// request's hidden bytes, and what verify says about its presentation. It needs the built
    /// programs, so run these with plain `cargo test`: a filter such as `--bin attest` skips them.
    async fn session_hiding(credential: &str, position: u64) -> Result<Value> {
        let dir = std::env::temp_dir().join(format!("hyalos-attest-hidden-{}-{position}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let mut rig = Rig(dir.clone(), Vec::new());
        let root_file = dir.join("root.der");
        // Beside this test binary's directory, where cargo builds the crate's programs.
        let built = env::current_exe()?.parent().and_then(|deps| deps.parent()).context("no build directory")?.to_path_buf();
        ensure!(built.join("standin").exists() && built.join("notary").exists(), "the stand-in or the notary is not built");
        let mut standin = Command::new(built.join("standin")).arg(&root_file).arg("provider.test").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
        let mut line = String::new();
        std::io::BufReader::new(standin.stdout.take().context("no output")?).read_line(&mut line)?;
        rig.1.push(standin);
        let address = line.trim().strip_prefix("listening ").context("the stand-in did not start")?.to_string();
        let me = Identity::make_or_load(&dir.join("operator.key"))?;
        let mut child = Command::new(built.join("notary"))
            .args(["--listen", "127.0.0.1:0", "--approved-server", "provider.test", "--stop-on-stdin-close", "--operator-transport-public", &me.public, "--dir"])
            .arg(dir.join("notary"))
            .arg("--root")
            .arg(&root_file)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let mut printed = std::io::BufReader::new(child.stdout.take().context("no output")?);
        rig.1.push(child);
        let mut field = |name: &str| -> Result<String> {
            let mut line = String::new();
            printed.read_line(&mut line)?;
            Ok(line.trim().strip_prefix(name).context("the notary did not start")?.trim().to_string())
        };
        let (signer, transport, notary_address) = (field("signing")?, field("transport")?, field("listening")?);
        let mut roots = tlsn::webpki::RootCertStore::mozilla();
        roots.roots.push(tlsn::webpki::CertificateDer(fs::read(&root_file)?));
        let trust = Trust { server: "provider.test".to_string(), roots, address, test: true };
        let run = "a".repeat(64);
        let carried = attest::binding(&run, position)?;
        let notary = NotaryLink { address: notary_address, transport, signer: signer.clone(), me };
        let broke = Arc::new(AtomicBool::new(false));
        let attested = tokio::time::timeout(Duration::from_secs(120), ask("A title", &carried, &run, &trust, credential, &notary, &broke)).await;
        let (_, presentation) = attested.map_err(|_| anyhow!("the attested call did not finish in 120 seconds"))??;
        verify(&presentation, &signer, &trust)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn issue_394_a_hidden_line_break_and_body_after_the_credential_is_refused() {
        // The control: a credential of the pinned length passes, so the refusal below is the length's.
        let clean = "x".repeat(CREDENTIAL_LENGTH);
        assert_eq!(session_hiding(&clean, 0).await.unwrap()["position"], 0);
        // A cheating operator's request: the credential, a blank line, then a body of their own.
        // The provider would read that body, and verify would show only the approved one.
        let smuggled = format!("{clean}\r\n\r\n{{\"model\":\"another\"}}");
        let refusal = session_hiding(&smuggled, 1).await.unwrap_err().to_string();
        assert!(refusal.contains("not a credential's length"), "{refusal}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn issue_394_a_hidden_run_one_byte_short_is_refused() {
        let short = "x".repeat(CREDENTIAL_LENGTH - 1);
        let refusal = session_hiding(&short, 2).await.unwrap_err().to_string();
        assert!(refusal.contains("not a credential's length"), "{refusal}");
    }
}
