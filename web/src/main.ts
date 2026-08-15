import './styles.css';
import { mount } from 'svelte';
import App from './App.svelte';

const target = document.getElementById('app');

if (!target) {
  throw new Error('PicManager application root is missing');
}

const app = mount(App, { target });

export default app;
