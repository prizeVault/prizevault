// Every variable the app reads, with safe defaults. Mirrors .env.example.

export const TESTNET_PASSPHRASE = "Test SDF Network ; September 2015";
export const TESTNET_RPC_URL = "https://soroban-testnet.stellar.org";

export interface Config {
  rpcUrl: string;
  networkPassphrase: string;
  contractId: string;
  /** Secret key of the signer (organizer, sponsor, player or referee depending on the command). */
  secretKey: string | undefined;
  /** SEP-41 token used by `create` when --token is not given. */
  tokenContractId: string | undefined;
  tokenDecimals: number;
  tokenSymbol: string;
}

export function loadConfig(env: Record<string, string | undefined> = process.env, overrides: Partial<Config> = {}): Config {
  const decimals = env.TOKEN_DECIMALS !== undefined ? Number(env.TOKEN_DECIMALS) : 7;
  if (!Number.isInteger(decimals) || decimals < 0 || decimals > 18) {
    throw new Error(`TOKEN_DECIMALS must be an integer between 0 and 18, got "${env.TOKEN_DECIMALS}"`);
  }
  return {
    rpcUrl: overrides.rpcUrl ?? env.STELLAR_RPC_URL ?? TESTNET_RPC_URL,
    networkPassphrase: overrides.networkPassphrase ?? env.STELLAR_NETWORK_PASSPHRASE ?? TESTNET_PASSPHRASE,
    contractId: overrides.contractId ?? env.PRIZEVAULT_CONTRACT_ID ?? "",
    secretKey: overrides.secretKey ?? env.STELLAR_SECRET_KEY,
    tokenContractId: overrides.tokenContractId ?? env.TOKEN_CONTRACT_ID,
    tokenDecimals: overrides.tokenDecimals ?? decimals,
    tokenSymbol: overrides.tokenSymbol ?? env.TOKEN_SYMBOL ?? "USDC",
  };
}
