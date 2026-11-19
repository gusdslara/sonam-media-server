sub init()
    m.top.functionName = "execute"
end sub

sub execute()
    request = CreateObject("roUrlTransfer")
    port = CreateObject("roMessagePort")
    request.SetMessagePort(port)
    request.SetUrl(m.top.url)
    request.SetCertificatesFile("common:/certs/ca-bundle.crt")
    if m.top.token <> "" then request.AddHeader("Authorization", "Bearer " + m.top.token)
    request.AddHeader("Content-Type", "application/json")
    if m.top.download
        started = request.AsyncGetToFile("tmp:/sonam-photo")
    else if m.top.method = "GET"
        started = request.AsyncGetToString()
    else
        request.SetRequest(m.top.method)
        started = request.AsyncPostFromString(m.top.body)
    end if
    if started
        event = wait(20000, port)
        if type(event) = "roUrlEvent"
            data = ParseJson(event.GetString())
            if data = invalid then data = {}
            m.top.result = { tag: m.top.tag, status: event.GetResponseCode(), data: data }
            return
        end if
    end if
    request.AsyncCancel()
    m.top.result = { tag: m.top.tag, status: 0, data: { error: "Servidor indisponivel. Confira o IP e a rede." } }
end sub
