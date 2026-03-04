import type { Address } from 'viem';
import {
  configureNetworks,
  getNetworks,
  resolveRpcUrl,
  tangleLocal,
  tangleMainnet,
  tangleTestnet,
  type CoreAddresses,
} from '@tangle-network/blueprint-ui';

export type OpenClawAddresses = CoreAddresses;

const ZERO_ADDRESS = '0x0000000000000000000000000000000000000000';
const defaultRpcPort = import.meta.env.VITE_RPC_PORT?.trim() || '8745';
const fallbackRpcUrl =
  typeof window === 'undefined'
    ? `http://127.0.0.1:${defaultRpcPort}`
    : `${window.location.protocol === 'https:' ? 'https' : 'http'}://${window.location.hostname}:${defaultRpcPort}`;
const localRpcUrl = resolveRpcUrl(import.meta.env.VITE_RPC_URL ?? fallbackRpcUrl);

configureNetworks<OpenClawAddresses>({
  [tangleLocal.id]: {
    chain: tangleLocal,
    rpcUrl: localRpcUrl,
    label: 'Tangle Local',
    shortLabel: 'Local',
    addresses: {
      jobs: (import.meta.env.VITE_JOBS_ADDRESS ?? import.meta.env.VITE_TANGLE_CONTRACT ?? ZERO_ADDRESS) as Address,
      services: (import.meta.env.VITE_SERVICES_ADDRESS ?? import.meta.env.VITE_TANGLE_CONTRACT ?? ZERO_ADDRESS) as Address,
    },
  },
  [tangleTestnet.id]: {
    chain: tangleTestnet,
    rpcUrl: 'https://testnet-rpc.tangle.tools',
    label: 'Tangle Testnet',
    shortLabel: 'Testnet',
    addresses: {
      jobs: (import.meta.env.VITE_TESTNET_JOBS_ADDRESS ?? ZERO_ADDRESS) as Address,
      services: (import.meta.env.VITE_TESTNET_SERVICES_ADDRESS ?? ZERO_ADDRESS) as Address,
    },
  },
  [tangleMainnet.id]: {
    chain: tangleMainnet,
    rpcUrl: 'https://rpc.tangle.tools',
    label: 'Tangle Mainnet',
    shortLabel: 'Mainnet',
    addresses: {
      jobs: (import.meta.env.VITE_MAINNET_JOBS_ADDRESS ?? ZERO_ADDRESS) as Address,
      services: (import.meta.env.VITE_MAINNET_SERVICES_ADDRESS ?? ZERO_ADDRESS) as Address,
    },
  },
});

export const networks = getNetworks<OpenClawAddresses>();
