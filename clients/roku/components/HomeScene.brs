sub init()
    m.top.backgroundColor = "0x17251eff"
    m.message = m.top.findNode("message")
    m.list = m.top.findNode("library")
    m.video = m.top.findNode("video")
    m.audio = m.top.findNode("audio")
    m.photo = m.top.findNode("photo")
    m.poll = m.top.findNode("poll")
    m.list.observeField("itemSelected", "selected")
    m.video.observeField("state", "playerState")
    m.audio.observeField("state", "playerState")
    m.poll.observeField("fire", "pollPair")
    m.registry = CreateObject("roRegistrySection", "sonam")
    m.server = m.registry.Read("server")
    m.token = m.registry.Read("token")
    m.compatible = false
    m.current = invalid
    if m.server = "" then configure() else loadLibrary()
end sub

sub configure()
    m.poll.control = "stop"
    dialog = CreateObject("roSGNode", "KeyboardDialog")
    dialog.title = "IP do Sonam (exemplo: http://192.168.1.10:8096)"
    dialog.text = m.server
    dialog.buttons = ["Conectar", "Cancelar"]
    dialog.observeField("buttonSelected", "serverChosen")
    m.top.dialog = dialog
end sub

sub serverChosen()
    dialog = m.top.dialog
    if dialog.buttonSelected <> 0
        dialog.close = true
        return
    end if
    address = dialog.text.Trim()
    pattern = CreateObject("roRegex", "^http://(10\.[0-9]+\.[0-9]+\.[0-9]+|192\.168\.[0-9]+\.[0-9]+|172\.(1[6-9]|2[0-9]|3[01])\.[0-9]+\.[0-9]+)(:[0-9]+)?/?$", "")
    if not pattern.IsMatch(address)
        dialog.title = "Use um IP local: http://192.168.1.10:8096"
        return
    end if
    if address.Right(1) = "/" then address = address.Left(Len(address)-1)
    if address <> m.server then m.token = ""
    m.server = address
    m.registry.Write("server", m.server)
    m.registry.Write("token", m.token)
    m.registry.Flush()
    dialog.close = true
    loadLibrary()
end sub

sub callApi(path as string, method as string, tag as string, body = "" as string, download = false as boolean)
    task = CreateObject("roSGNode", "ApiTask")
    task.url = m.server + "/api" + path
    task.method = method
    task.token = m.token
    task.tag = tag
    task.body = body
    task.download = download
    task.observeField("result", "apiResult")
    m.task = task
    task.control = "run"
end sub

sub loadLibrary()
    m.list.visible = false
    m.message.text = "Abrindo sua colecao..."
    if m.token = ""
        callApi("/pair", "POST", "pair")
    else
        callApi("/media", "GET", "library")
    end if
end sub

sub pollPair()
    if m.task.state <> "run" then callApi("/pair/" + m.pairId, "POST", "poll")
end sub

sub apiResult(event as object)
    result = event.GetData()
    if result.status = 401
        m.token = ""
        m.registry.Delete("token")
        m.registry.Flush()
        loadLibrary()
        return
    end if
    if result.status <> 200
        m.poll.control = "stop"
        m.message.text = "Nao foi possivel conectar. Pressione Voltar para tentar outro IP."
        if result.data.error <> invalid then m.message.text = result.data.error + "  Pressione Voltar para tentar novamente."
        return
    end if
    if result.tag = "pair"
        m.pairId = result.data.id
        m.message.text = "Na sua conta do Sonam, abra Parear tela e digite: " + result.data.pin + chr(10) + "Codigo valido por 5 minutos. Esta TV tera as permissoes do perfil que autorizar."
        m.poll.control = "start"
    else if result.tag = "poll"
        if result.data.approved
            m.poll.control = "stop"
            m.token = result.data.token
            m.registry.Write("token", m.token)
            m.registry.Flush()
            loadLibrary()
        end if
    else if result.tag = "library"
        m.items = result.data
        content = CreateObject("roSGNode", "ContentNode")
        for each item in m.items
            node = content.CreateChild("ContentNode")
            node.title = "[" + item.kind + "]  " + item.name
        end for
        m.list.content = content
        m.list.visible = true
        m.list.setFocus(true)
        m.message.text = "Sua biblioteca pessoal. " + m.items.Count().ToStr() + " arquivos. Modo: original."
    else if result.tag = "photo"
        m.photo.uri = ""
        m.photo.uri = "tmp:/sonam-photo"
        m.photo.visible = true
        m.list.visible = false
    end if
end sub

sub selected()
    if m.items = invalid then return
    item = m.items[m.list.itemSelected]
    m.current = item
    if item.kind = "photo"
        callApi("/media/" + item.id + "/stream", "GET", "photo", "", true)
        return
    end if
    content = CreateObject("roSGNode", "ContentNode")
    content.title = item.name
    content.url = m.server + "/api/media/" + item.id + "/stream"
    content.httpHeaders = ["Authorization:Bearer " + m.token]
    if m.compatible
        content.url = m.server + "/api/media/" + item.id + "/compatible"
        content.streamFormat = "mp4"
    else if item.kind = "video"
        content.streamFormat = "mp4"
    else if item.mime = "audio/mpeg"
        content.streamFormat = "mp3"
    else if item.mime = "audio/flac"
        content.streamFormat = "flac"
    else if item.mime = "audio/wav"
        content.streamFormat = "wav"
    else
        content.streamFormat = "mp4"
    end if
    player = m.audio
    if item.kind = "video" then player = m.video
    agent = CreateObject("roHttpAgent")
    player.setHttpAgent(agent)
    player.content = content
    if item.kind = "video"
        m.video.visible = true
        m.video.setFocus(true)
    else
        m.message.text = "Tocando: " + item.name
    end if
    player.control = "play"
end sub

sub playerState(event as object)
    if event.GetData() = "error"
        m.video.visible = false
        m.list.setFocus(true)
        m.message.text = "Formato incompativel. Pressione * para alternar o modo e OK para tentar de novo. Conversao exige FFmpeg no servidor."
    else if event.GetData() = "finished"
        m.video.visible = false
        m.list.setFocus(true)
    end if
end sub

function onKeyEvent(key as string, press as boolean) as boolean
    if not press then return false
    if key = "options"
        m.compatible = not m.compatible
        mode = "original"
        if m.compatible then mode = "compativel (FFmpeg)"
        m.message.text = "Modo de reproducao: " + mode + ". Selecione o arquivo para reproduzir."
        return true
    end if
    if key = "back"
        if m.video.visible or m.photo.visible or m.audio.state = "playing"
            m.video.control = "stop"
            m.audio.control = "stop"
            m.video.visible = false
            m.photo.visible = false
            m.list.visible = true
            m.list.setFocus(true)
        else
            configure()
        end if
        return true
    end if
    return false
end function
