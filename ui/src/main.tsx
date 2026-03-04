import './polyfills';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import 'virtual:uno.css';
import './styles/global.scss';
import App from './App.tsx';
import './lib/contracts/chains';
import { Web3Provider } from './providers/Web3Provider';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Web3Provider>
      <App />
    </Web3Provider>
  </StrictMode>,
);
