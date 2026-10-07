import { createApp } from 'vue';
import App from './App.vue';
import Companion from './Companion.vue';
import './style.css';

const companion = new URLSearchParams(location.search).get('window') === 'companion';
document.documentElement.dataset.window = companion ? 'companion' : 'main';
createApp(companion ? Companion : App).mount('#app');
