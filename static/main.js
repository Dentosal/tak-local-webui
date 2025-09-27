function clone(obj) {
    return JSON.parse(JSON.stringify(obj));
}

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
var ponderPerPly = {};
var ponderSub = null;
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
            if (state.isGameEnd) {
                if (document.getElementById("opt-training-mode").checked) {
                    let elem = document.getElementById("analysis");
                    let elemScore = document.getElementById("analysis-score");
                    let elemText = document.getElementById("analysis-text");

                    elemScore.innerText = state.result.text;

                    if (state.result.winner === playingAs) {
                        elemText.innerText = "Victory!";
                    } else {
                        elemText.innerText = "Possibly " + ponderPerPly[plyIndex - 1].pv[0];
                    }
                }
            } else {
                if (document.getElementById("opt-training-mode").checked) {
                    await ponder(clone(state));
                }
                if (playingAs && state.turn !== playingAs) {
                    await playAiMove();
                }
            }
            break;
        case "GAME_END":
            if (document.getElementById("autorestart").checked) {
                setTimeout(restartGame, 500);
            }
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

async function getAiAnalysis() {
    let engine = document.getElementById("opt-ai").value;
    let reply = await fetch(
        "/bestmove?engine=" + engine + "&tps=" + encodeURIComponent(state.tps),
    );
    if (!reply.ok) {
        console.error("Failed to get AI move:", reply.statusText);
        return null;
    }
    let data = await reply.json();
    console.log("AI move:", data);
    return data;
}

async function playAiMove() {
    let trainingMode = document.getElementById("opt-training-mode").checked;

    document.getElementById("ai-state").innerText = "thinking...";
    let move = await getAiAnalysis();
    document.getElementById("ai-state").innerText = "idle"

    if (move) {
        console.log("Playing AI move:", move.bestmove, "in state", state);
        sendAction("APPEND_PLY", move.bestmove);
    }
}

async function startGame() {
    if (ponderSub) {
        ponderSub.close();
        ponderSub = null;
    }
    ponderPerPly = {};
    document.getElementById("ai-state").innerText = "idle";
    document.getElementById("analysis").style.backgroundColor = "initial";
    document.getElementById("analysis-score").innerText = "";
    document.getElementById("analysis-text").innerText = "";

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
}

async function resetGame() {
    document.querySelector(".overlay").style.display = "flex";
}

// Reset the game and start again with same settings
async function restartGame() {
    startGame();
}

async function ponder(ponderState) {
    if (ponderSub) {
        let s2 = ponderSub;
        setTimeout(() => s2.close(), 1000);
    }

    let plyIndex = ponderState.plyIndex;
    let tps = ponderState.tps;
    let isPlayerMove = ponderState.turn === playingAs;


    ponderSub = new EventSource("/ponder?engine=" + document.getElementById("opt-ai").value + "&tps=" + encodeURIComponent(tps));
    ponderSub.onerror = async (e) => {
        console.error("Ponder event source error:", e);
    };
    ponderSub.addEventListener("error", async (e) => {
        console.error("Ponder error:", e.data);
    });
    ponderSub.addEventListener("info", async (e) => {
        ponderPerPly[plyIndex] = JSON.parse(e.data);

        if (plyIndex !== state.plyIndex || state.isGameEnd) {
            // Ponder info is for an earlier ply, ignore
            return;
        }

        let elem = document.getElementById("analysis");
        let elemScore = document.getElementById("analysis-score");
        let elemText = document.getElementById("analysis-text");
        
        let playerScoreNow = ponderPerPly[plyIndex].score * (isPlayerMove ? 1 : -1);
        elemScore.innerText = playerScoreNow;

        if (plyIndex > 1 && !isPlayerMove) {
            let playerScorePrev = ponderPerPly[plyIndex - 1].score;
            let lostScore = playerScorePrev - playerScoreNow;

            let pv = ponderPerPly[plyIndex - 1].pv[0];
            if (pv === state.ply) {
                // The player played the move the engine suggested
                elemText.innerText = "Best move!";
                if (playerScoreNow <= -80) {
                    elemText.innerText += " (Still losing position...)";
                }
            } else if (playerScoreNow <= -80) {
                elemText.innerText = "Losing position!";
            } else if (lostScore > 20) {
                elemText.innerText = "Blunder! " + pv + " was better";
            } else if (lostScore > 5) {
                elemText.innerText = "Weak! " + pv + " was better";
            } else if (lostScore > -5) {
                elemText.innerText = "Ok " + pv + " was top choice";
            } else {
                elemText.innerText = "Strong!! " + pv + " was best";
            }
        }
    });
}