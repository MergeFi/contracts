export const API_URL =
  process.env.NEXT_PUBLIC_API_URL ?? 'http://localhost:3001/api/v1';

export const STELLAR_NETWORK =
  (process.env.NEXT_PUBLIC_STELLAR_NETWORK ?? 'TESTNET') as 'TESTNET' | 'PUBLIC';

export const HORIZON_URL =
  process.env.NEXT_PUBLIC_HORIZON_URL ??
  (STELLAR_NETWORK === 'PUBLIC'
    ? 'https://horizon.stellar.org'
    : 'https://horizon-testnet.stellar.org');

export const SOROBAN_RPC_URL =
  process.env.NEXT_PUBLIC_SOROBAN_RPC_URL ??
  (STELLAR_NETWORK === 'PUBLIC'
    ? 'https://soroban.stellar.org'
    : 'https://soroban-testnet.stellar.org');

export const USDC_ASSET_CODE   = 'USDC';
export const USDC_ISSUER_TESTNET = 'GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5';
export const USDC_ISSUER =
  STELLAR_NETWORK === 'PUBLIC'
    ? 'GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN'
    : USDC_ISSUER_TESTNET;

export const STROOPS_PER_UNIT = 10_000_000n;

export const POLICY_CONTRACT_ID =
  process.env.NEXT_PUBLIC_POLICY_CONTRACT_ID ?? '';

export const ORACLE_CONTRACT_ID =
  process.env.NEXT_PUBLIC_ORACLE_CONTRACT_ID ?? '';

export const CLAIMS_CONTRACT_ID =
  process.env.NEXT_PUBLIC_CLAIMS_CONTRACT_ID ?? '';

export const CATEGORY_LABELS: Record<string, string> = {
  crop:     'Crop Insurance',
  flight:   'Flight Delay',
  disaster: 'Natural Disaster',
  health:   'Health',
  defi:     'DeFi Cover',
};

export const CATEGORY_ICONS: Record<string, string> = {
  crop:     '🌾',
  flight:   '✈️',
  disaster: '🌪️',
  health:   '🏥',
  defi:     '🔐',
};

export const STATUS_COLOURS: Record<string, string> = {
  Active:     'emerald',
  Expired:    'gray',
  Claimed:    'sky',
  Cancelled:  'red',
  Pending:    'amber',
  Processing: 'sky',
  Paid:       'emerald',
  Rejected:   'red',
};

export const WALLET_STORAGE_KEY   = 'ps_wallet_id';
export const ADDRESS_STORAGE_KEY  = 'ps_wallet_address';
export const NETWORK_STORAGE_KEY  = 'ps_wallet_network';
export const AUTH_TOKEN_STORAGE_KEY = 'ps_auth_token';

export const TOAST_DEFAULT_DURATION_MS = 4000;
export const POLLING_INTERVAL_MS       = 30_000;
export const ORACLE_REFRESH_INTERVAL_MS = 60_000;
