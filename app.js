// --- DOM Elements ---
const setupScreen = document.getElementById("setup-screen");
const gameScreen = document.getElementById("game-screen");
const playerCountSelect = document.getElementById("player-count");
const lifePointsInput = document.getElementById("life-points");
const startGameButton = document.getElementById("start-game");
const resetButton = document.getElementById("reset-button");

// --- Player Colors ---
const playerColors = [
  "bg-red-500",
  "bg-blue-500",
  "bg-green-500",
  "bg-yellow-500",
  "bg-purple-500",
  "bg-pink-500",
  "bg-cyan-500",
  "bg-orange-500",
  "bg-lime-500",
];

// --- Game State ---
let longPressTimer;
let longPressOccurred = false;
const LONG_PRESS_DURATION = 500; // ms
let playerStates = [];
let wakeLock = null;
let gameStarted = false;

// --- Game Logic ---
function createPlayerSection(playerIndex, initialLife, playerCount) {
  const playerDiv = document.createElement("div");
  const colorClass = playerColors[playerIndex % playerColors.length];
  playerDiv.className = `player-container ${colorClass}`;
  playerDiv.dataset.player = playerIndex;

  // --- Rotation Logic ---
  let rotation = 0;
  if (playerCount >= 2) {
    if (2 * playerIndex < playerCount) rotation = 180;
  }
  playerDiv.style.transform = `rotate(${rotation}deg)`;

  let currentLife = initialLife;

  const lifeDisplay = document.createElement("div");
  lifeDisplay.className = "life-total";
  lifeDisplay.textContent = currentLife;

  const snackbar = document.createElement("div");
  snackbar.className = "change-snackbar";

  const minusArea = document.createElement("div");
  minusArea.className = "control-area minus-area";

  const plusArea = document.createElement("div");
  plusArea.className = "control-area plus-area";

  // --- Event Listeners for Life Changes ---
  const changeLife = (amount) => {
    currentLife += amount;
    lifeDisplay.textContent = currentLife;
    updateSnackbar(playerIndex, amount);
  };

  const handlePress = (amount) => {
    longPressOccurred = false;
    longPressTimer = setTimeout(() => {
      changeLife(amount * 5);
      longPressOccurred = true;
    }, LONG_PRESS_DURATION);
  };

  const handleRelease = (amount) => {
    clearTimeout(longPressTimer);
    if (!longPressOccurred) {
      changeLife(amount);
    }
  };

  minusArea.addEventListener("mousedown", () => handlePress(-1));
  minusArea.addEventListener("mouseup", () => handleRelease(-1));
  minusArea.addEventListener("mouseleave", () => clearTimeout(longPressTimer));
  minusArea.addEventListener("touchstart", (e) => {
    e.preventDefault();
    handlePress(-1);
  });
  minusArea.addEventListener("touchend", () => handleRelease(-1));

  plusArea.addEventListener("mousedown", () => handlePress(1));
  plusArea.addEventListener("mouseup", () => handleRelease(1));
  plusArea.addEventListener("mouseleave", () => clearTimeout(longPressTimer));
  plusArea.addEventListener("touchstart", (e) => {
    e.preventDefault();
    handlePress(1);
  });
  plusArea.addEventListener("touchend", () => handleRelease(1));

  playerDiv.appendChild(lifeDisplay);
  playerDiv.appendChild(snackbar);
  playerDiv.appendChild(minusArea);
  playerDiv.appendChild(plusArea);

  return playerDiv;
}

function updateSnackbar(playerIndex, amount) {
  const state = playerStates[playerIndex];
  const now = Date.now();

  clearTimeout(state.snackbarTimer);

  if (now - state.lastChangeTime < 3000) {
    state.totalChange += amount;
  } else {
    state.totalChange = amount;
  }
  state.lastChangeTime = now;

  const snackbar =
    gameScreen.children[playerIndex].querySelector(".change-snackbar");
  snackbar.textContent = (state.totalChange > 0 ? "+" : "") + state.totalChange;
  snackbar.classList.add("show");

  state.snackbarTimer = setTimeout(() => {
    snackbar.classList.remove("show");
  }, 2500);
}

function setupGameLayout(playerCount) {
  gameScreen.innerHTML = ""; // Clear previous game
  if (playerCount <= 3) {
    gameScreen.style.gridTemplateColumns = "1fr";
    gameScreen.style.gridTemplateRows = `repeat(${playerCount}, 1fr)`;
  } else {
    gameScreen.style.gridTemplateColumns = "1fr 1fr";
    gameScreen.style.gridTemplateRows = `repeat(${playerCount / 2}, 1fr)`;
  }
}

// --- Event Handlers ---
startGameButton.addEventListener("click", () => {
  gameStarted = true;
  enableWakeLock();
  const playerCount = parseInt(playerCountSelect.value, 10);
  const initialLife = parseInt(lifePointsInput.value, 10);

  // Reset and initialize states for each player
  playerStates = [];
  for (let i = 0; i < playerCount; i++) {
    playerStates.push({
      lastChangeTime: 0,
      totalChange: 0,
      snackbarTimer: null,
    });
  }

  setupGameLayout(playerCount);

  for (let i = 0; i < playerCount; i++) {
    const playerSection = createPlayerSection(i, initialLife, playerCount);
    gameScreen.appendChild(playerSection);
  }

  setupScreen.classList.add("hidden");
  gameScreen.classList.remove("hidden");
  gameScreen.classList.add("grid");
  resetButton.classList.remove("hidden");
});

resetButton.addEventListener("click", () => {
  gameStarted = false;
  releaseWakeLock();
  gameScreen.classList.add("hidden");
  gameScreen.classList.remove("grid");
  resetButton.classList.add("hidden");
  setupScreen.classList.remove("hidden");
});

if ("serviceWorker" in navigator) {
  navigator.serviceWorker.register("sw.js");
}

async function enableWakeLock() {
  if (gameStarted) {
    try {
      wakeLock = await navigator.wakeLock.request("screen");
    } catch (err) {}
  }
}

async function releaseWakeLock() {
  if (wakeLock === null) return;
  try {
    wakeLock.release();
  } catch (err) {}
}

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") {
    enableWakeLock();
  }
});
