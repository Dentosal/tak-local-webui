var ninja = null;
var ready = false;
window.onload = () => {
    ninja = document.getElementById("ninja");
    ninja.src = "https://ptn.ninja/" + encodeURIComponent('[Size "5"]\n\n') + "&verticalLayout=false&disableNavigation=true&disablePTNTools=true";
    window.addEventListener("message", messageHandler, false);

    fetch("/engines").then(res => res.json()).then(engines => {
        const select = document.getElementById("opt-ai");
        engines.forEach(engine => {
            const option = document.createElement("option");
            option.value = engine;
            option.text = engine;
            select.appendChild(option);
        });
    });
};

var playingAs = null;
var state = null;
async function messageHandler(event) {
    if (event.source !== ninja.contentWindow) {
        return;
    }
    const { action, value } = event.data;
    console.log("Received message:", action, value);
    // Consider the PTN Ninja embed loaded after first GAME_STATE message
    if (!ready) {
        if (action === "GAME_STATE") {
            ready = true;
        }
        else {
            return; // Ignore other messages until ptn.ninja is fully loaded
        }
    }
    switch (action) {
        case "GAME_STATE":
            state = value;
            await updateUI();
            if (playingAs && state.turn !== playingAs) {
                await playAiMove();
            }
            break;
        case "INSERT_END":
            break;
        case "INSERT_PLY":
            break;
        default:
            break;
    }
}

function sendAction(action, value) {
    ninja.contentWindow.postMessage({
        action,
        value
    }, '*');
}

async function updateUI(event) {
}

async function getAiMove() {
    let engine = document.getElementById("opt-ai").value;
    let reply = await fetch(
        "/bestmove?engine=" + engine + "&tps=" + encodeURIComponent(state.tps),
    );
    if (!reply.ok) {
        console.error("Failed to get AI move:", reply.statusText);
        return null;
    }
    let data = await reply.text();
    console.log("AI move:", data);
    return data;
}

async function playAiMove() {
    document.getElementById("ai-state").innerText = "thinking...";
    let move = await getAiMove();
    document.getElementById("ai-state").innerText = "idle";
    if (move) {
        console.log("Playing AI move:", move, "in state", state);
        sendAction("APPEND_PLY", move);
    }
}

async function startGame() {
    let pickedColor = document.querySelector("input[name=opt-color]:checked").value;
    if (pickedColor !== "white" && pickedColor !== "black") {
        console.error("Invalid color selected");
        return;
    }
    let boardSize = document.querySelector("input[name=opt-board-size]:checked").value;
    sendAction("SET_CURRENT_PTN", '[Size "' + boardSize + '"]\n\n');
    playingAs = pickedColor === 'white' ? 1 : 2;
    sendAction("SET_PLAYER", playingAs);
    document.querySelector(".overlay").style.display = "none";
    updateUI();
}

async function resetGame() {
    document.querySelector(".overlay").style.display = "flex";
}
