const GATEWAY = process.env.NEXT_PUBLIC_GATEWAY_URL ?? "http://localhost:8080";
const DB_API   = process.env.NEXT_PUBLIC_DB_API_URL  ?? "http://localhost:3001";

export type Job = {
  id:               string;
  status:           string;
  input_hash:       string;
  proof_path:       string | null;
  error:            string | null;
  submitted_at:     string;
  started_at:       string | null;
  completed_at:     string | null;
  settled_at:       string | null;
  tx_hash:          string | null;
  attestation_hash: string | null;
};

export type Bet = {
  id:               string;
  job_id:           string | null;
  market_id:        string;
  question:         string;
  side:             "YES" | "NO";
  size_usdc:        number;
  price:            number;
  paper:            boolean;
  confidence:       number;
  yes_price:        number;
  no_price:         number;
  volume_24h:       number;
  attestation_hash: string | null;
  tx_hash:          string | null;
  outcome:          boolean | null;
  pnl_usdc:         number | null;
  placed_at:        string;
  resolved_at:      string | null;
};

export async function fetchJobs(limit = 50): Promise<Job[]> {
  const r = await fetch(`${DB_API}/jobs?limit=${limit}`, { cache: "no-store" });
  if (!r.ok) throw new Error("Failed to fetch jobs");
  return r.json();
}

export async function fetchBets(limit = 50): Promise<Bet[]> {
  const r = await fetch(`${DB_API}/bets?limit=${limit}`, { cache: "no-store" });
  if (!r.ok) throw new Error("Failed to fetch bets");
  return r.json();
}

export async function fetchJob(id: string): Promise<Job> {
  const r = await fetch(`${GATEWAY}/v1/jobs/${id}`, { cache: "no-store" });
  if (!r.ok) throw new Error("Failed to fetch job");
  return r.json();
}