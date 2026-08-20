# Ejemplo del contexto que recibe el LLM — Microsoft, Azure/modelos

Corrida capturada: k=12, sin embeddings, motor local. Este es EXACTAMENTE el prompt enviado.

> Nota posterior: esta corrida (k=12) fue rechazada por el servidor con `-c 8192`
> (el prompt pesaba 8326 tokens > 8192). El servidor se lanzó con `-c 16384` y el
> problema desapareció: con `k=8` el modelo Qwen3.8-4B (default) extrae ambas
> cifras — $75 mil millones (pág. 2) y más de 11.000 modelos (pág. 3).

## 1. System prompt

```
Eres un analista financiero sénior especializado en memorias anuales y reportes estadísticos del mercado peruano.
Tu misión es responder con máxima fidelidad, precisión ejecutiva y estricto apego a las fuentes.

REGLAS INVIOLABLES:
1. IDENTIFICACIÓN Y GROUNDING POR MÉTRICA: Para cada métrica que pide la pregunta, identifica su cifra por el CALIFICADOR (entidad, periodo, moneda/columna) y cítala con su fuente. Si el contexto contiene la misma métrica para OTRA entidad, año, moneda o columna (p.ej. 'mora de Efectiva' vs 'mora del Sistema Financiero'; 'liquidez 2025' vs 'liquidez 2024'), cita la que corresponde a la pregunta, nunca la otra. Si una métrica pedida no figura o no se distingue cuál corresponde, indica '[Dato no especificado en los documentos]' SOLO para esa métrica y continúa con las demás.
2. CERO ALUCINACIONES NUMÉRICAS: NUNCA deduzcas, inventes, redondees, conviertas de moneda ni aproximes números, porcentajes o monedas que no estén textualmente en el contexto.
3. CITAS PRECISAS: Cada cifra relevante debe citar el documento y la página correspondiente entre corchetes, ej: [Ferreycorp_Memoria_2025 pág. 39].
4. IDIOMA: Responde siempre en español formal y directo.
```

## 2. User prompt (contexto + pregunta)

```
--- DOCUMENTOS Y CONTEXTO FUENTE ---


### FUENTE [1]
DOCUMENTO: Microsoft_annual_report_2025 (pág. 21)
CONTENIDO:
[fragmento relevante: ents. We hold rights to OpenAI’s intellectual 

property, including models and infrastructure, for integration into our products. The OpenAI API is exclusive to Azure, runs 

on Azure, and is ava]
20 

MANAGEMENT’S DISCUSSION AND ANALYSIS OF FINANCIAL CONDITION AND RESULTS  

OF OPERATIONS  

The following Management’s Discussion and Analysis of Financial Condition and Results of Operations (“MD&A”) is 

intended to help the reader understand the results of operations and financial condition of Microsoft Corporation. MD&A is 

provided as a supplem ent to, and should be read in conjunction with, our consolidated financial statements and the 

accompanying Notes to Financial Statements. This section generally discusses the results of our operations for the year 

ended June  30, 2025 compared to the year e nded June  30, 2024. For a discussion of the year ended June  30, 2024 

compared to the year ended June 30, 2023, please refer to “Management’s Discussion and Analysis of Financial Condition 

and Results of Operations” in our Annual Report on Form 10-K for the year ended June 30, 2024 and our Form 8-K filed on 

December 3, 2024.  

OVERVIEW  

Microsoft is a technology company committed to making digital technology and artificial intelligence (“AI”) available broadly  

and doing so responsibly, with a mission to empower every person and every organization on the planet to achieve more. 

We create p latforms and tools, powered by AI, that deliver innovative solutions that meet the evolving needs of our 

customers.  

We generate revenue by offering a wide range of cloud -based solutions, content, and other services to people and 

businesses; licensing and supporting an array of software products; delivering relevant online advertising to a global 

audience; and designing and selling devices. Our most significant expenses are related to compensating employees; 

supporting and investing in our cloud-based services, including datacenter operations; designing, manufacturing, marketing, 

and selling our other products and services; and income taxes.  

Highlights from fiscal year 2025 compared with fiscal year 2024 included:  

• Microsoft Cloud

designing, manufacturing, marketing, 

and selling our other products and services; and income taxes.  

Highlights from fiscal year 2025 compared with fiscal year 2024 included:  

• Microsoft Cloud revenue increased 23% to $168.9 billion.  

• Microsoft 365 Commercial products and cloud services revenue increased 14% driven by Microsoft 365 

Commercial cloud revenue growth of 15%.  

• Microsoft 365 Consumer products and cloud services revenue increased 11% driven by Microsoft 3 65 

Consumer cloud revenue growth of 11%.  

• LinkedIn revenue increased 9%.  

• Dynamics products and cloud services revenue increased 15% driven by Dynamics 365 revenue growth of 

19%.  

• Server products and cloud services revenue increased 23% driven by Azure and other cloud services revenue 

growth of 34%.  

• Windows OEM and Devices revenue increased 3%.  

• Xbox content and services revenue increased 16%.  

• Search and news advertising revenue excluding traffic acquisition costs increased 20%.  

Industry Trends and Opportunities  

Our industry is dynamic and highly competitive, with frequent changes in both technologies and business models. Each 

industry shift is an opportunity to conceive new products, new technologies, or new ideas that can further transform the 

industry and our business. At Microsoft, we push the boundaries of what is possible through a broad range of research and 

development activities that seek to identify and address the changing demands of customers and users, industry trends, 

and competitive forces.  

Microsoft and OpenAI maintain a long-term strategic partnership originally established in 2019. Microsoft is a major investor 

in OpenAI, and the companies have reciprocal revenue -sharing arrangements. We hold rights to OpenAI’s intellectual 

property, including models and infrastructure, for integration into our products. The OpenAI API is exclusive to Azure, runs 

on Azure, and is ava

angements. We hold rights to OpenAI’s intellectual 

property, including models and infrastructure, for integration into our products. The OpenAI API is exclusive to Azure, runs 

on Azure, and is available through the Azure OpenAI Service. We also have a right of first refusal on OpenAI’s new capacity 

needs.

### FUENTE [2]
DOCUMENTO: Microsoft_annual_report_2025 (pág. 2)
CONTENIDO:
[fragmento relevante: eing guided by our bold vision for the future.  

Financially, it was a year of record performance. Revenue was $281.7  billion, up 15  percent. Operating income grew 

17 percent to $128.5  billion. And Azure surpassed $75  billion in revenue for the first time, up 34  percent. These results]
1 

Dear shareholders, colleagues, customers, and partners:  

Fifty years after our founding, Microsoft is once again at the heart of a generational moment in technology as we find 

ourselves in the midst of the AI platform shift. More than any transformation before it, this generation of AI is radically 

changing every layer of the tech stack, and we are changing with it.  

Across the company, we are accelerating our pace of innovation and adapting to both a new tech stack and a new way of 

working. We are delivering our current platforms at scale while building the next  generation, always striving to create more 

value for our customers, our partners, and the world.  

Striking this balance is hard work, and few companies over the years have been able to do it. To succeed, we must continue 

to think in decades but execute in  quarters, approaching each day with the humility and curiosity required to continuously 

improve, while being guided by our bold vision for the future.  

Financially, it was a year of record performance. Revenue was $281.7  billion, up 15  percent. Operating income grew 

17 percent to $128.5  billion. And Azure surpassed $75  billion in revenue for the first time, up 34  percent. These results 

reflect the growing demand for our platform and the trust customers are placing in us. We take neither for granted.  

We must earn our permission to operate every day, in every country, every community, and every customer interaction. 

That’s why we remain grounded in our mission:  to empower every person and every organization on the planet to 

achieve more.  

Imagine a world where every person can get help from a researcher, a coder, or an analyst on demand. Not just information, 

but deep, contextual expertise paired with action. Or where every organization, no matter its size or sector, can reinvent 

employee experiences, reimagine customer engagement, reshape business processes, and bend the curve on innovation 

fo

action. Or where every organization, no matter its size or sector, can reinvent 

employee experiences, reimagine customer engagement, reshape business processes, and bend the curve on innovation 

for their people, businesses, and industries. This is the new frontier and how we will unlock the next level of productivity and 

growth for the world.  

But it is not some far off vision—we are already seeing what’s possible when AI reaches the frontlines of human potential, 

helping small businesses become more productive, multinationals more competitive, nonprofits more effective, governments 

more efficient, and improving healthcare and education outcomes.  

To share just a few examples across industries: Mercy, one of the largest health systems in the US, has saved caregivers 

over 100,000 hours by automatically documenting physician -patient encounters. As one physician put i t: “the best thing to 

happen to my practice in 10 years.” A grandmother in Japan, who lost her hearing at age two, can now communicate with 

her voice, thanks to an AI app. A judge in Colombia is using Copilot to expedite due process and help tackle a backl og of 

court cases. Barclays Bank is putting AI in the hands of 100,000 employees, transforming the employee experience by 

simplifying how they access information and get things done. Ralph Lauren is helping customers find the perfect look for 

any occasion,  thanks to a new conversational shopping experience. Carvana has reduced inbound calls per sale by 

45 percent, freeing its staff to focus on complex, high-value support.  

These examples, and so many others like them, are made possible by our clear focus on our priorities, our responsibility, 

and our culture.  

OUR PRIORITIES  

To deliver on our mission, we remain focused on three core business priorities as our North Star: se curity, quality, and AI 

innovation.  

Security

sibility, 

and our culture.  

OUR PRIORITIES  

To deliver on our mission, we remain focused on three core business priorities as our North Star: se curity, quality, and AI 

innovation.  

Security and quality are non -negotiable. Our infrastructure and services are mission critical for the world. This year, we 

made significant progress across both our Secure Future Initiative (SFI) and Quality Excellence Initiative (QEI), but we 

recognize our work here is never done. We must continuously raise the bar for ourselves and our customers.  

Security  

Through SFI, we have dedicated the equivalent of 34,000 full -time engineers to our highest -priority security work . We 

strengthened identity protections, secured our networks and systems, enhanced threat detection and response, and 

embedded secure-by-design practices across everything we build.

### FUENTE [3]
DOCUMENTO: Microsoft_annual_report_2025 (pág. 69)
CONTENIDO:
[fragmento relevante: d CRM 

applications.  

Intelligent Cloud  

Our Intelligent Cloud segment consists of our public, private, a nd hybrid server products and cloud services that power 

modern business and developers. This segment primarily comprises:  

• Server products and cloud services, including Azure and other cloud services, comprising cloud and AI 

co]
68 

As of June 30, 2025, 62 million shares of our common stock were reserved for future issuance through the ESPP.  

Savings Plans  

We have savings plans in the U.S. that qualify under Section 401(k) of the Internal Revenue Code, and a number of savings 

plans in international locations. Eligible U.S. employees may contribute a portion of their salary into the savings plans, 

subject to certain limitations. We match a portion of each dollar a participant contributes into the plans. Employer -funded 

retirement benef its for all plans were $1.8  billion, $1.7  billion, and $1.6  billion in fiscal years 2025, 2024, and 2023, 

respectively, and were expensed as contributed.  

NOTE 18 — SEGMENT INFORMATION AND GEOGRAPHIC DATA  

In its operation of the business, management, including our chief operating decision maker (“CODM”), who is also our Chief 

Executive Officer, reviews certain financial information, including segmented internal profit and loss statements. The primary 

profitability measure used by the CODM to review segment operating results is operating income. The CODM uses 

operating income to allocate resources during our annual planning process and throughout the year, as well as to assess 

the performance of our segments,  primarily by monitoring actual results compared to prior periods and expected results. 

During the periods presented, we reported our financial performance based on the following segments: Productivity and 

Business Processes, Intelligent Cloud, and More Personal Computing.  

We have recast certain prior period amounts to conform to the way we internally manage and monitor our business. Refer 

to Note 1 – Accounting Policies for further information.  

Our reportable segments are described below.  

Productivity and Business Processes  

Our Productivity and Business Processes segment consists of products and services in our portfolio of productivity, 

communic

e segments are described below.  

Productivity and Business Processes  

Our Productivity and Business Processes segment consists of products and services in our portfolio of productivity, 

communication, and information services, spanning a variety of devices and platforms. This segment primarily comprise s:  

• Microsoft 365 Commercial products and cloud services, including Microsoft 365 Commercial cloud, comprising 

Microsoft 365 Commercial, Enterprise Mobility + Security, the cloud portion of Windows Commercial, the per -

user portion of Power BI, Exchange, ShareP oint, Microsoft Teams, Microsoft 365 Security and Compliance, 

and Microsoft 365 Copilot; and Microsoft 365 Commercial products, comprising Windows Commercial on -

premises and Office licensed on-premises.  

• Microsoft 365 Consumer products and cloud services, including Microsoft 365 Consumer subscriptions, Office 

licensed on-premises, and other consumer services.  

• LinkedIn, including Talent Solutions, Marketing Solutions, Premium Subscriptions, and Sales Solutions.  

• Dynamics products and cloud services, i ncluding Dynamics 365, comprising a set of intelligent, cloud -based 

applications across ERP, CRM, Power Apps, and Power Automate; and on -premises ERP and CRM 

applications.  

Intelligent Cloud  

Our Intelligent Cloud segment consists of our public, private, a nd hybrid server products and cloud services that power 

modern business and developers. This segment primarily comprises:  

• Server products and cloud services, including Azure and other cloud services, comprising cloud and AI 

consumption-based services, G itHub cloud services, Nuance Healthcare cloud services, virtual desktop 

offerings, and other cloud services; and Server products, comprising SQL Server, Windows Server, Visual 

Studio, System Center, related Client Access Licenses (“CALs”), and other on-premises offerings.  

• Enterprise a

cloud services; and Server products, comprising SQL Server, Windows Server, Visual 

Studio, System Center, related Client Access Licenses (“CALs”), and other on-premises offerings.  

• Enterprise and partner services, including Enterprise Support Services, Industry Solutions, Nuance 

professional services, Microsoft Partner Network, and Learning Experience.

### FUENTE [4]
DOCUMENTO: Microsoft_annual_report_2025 (pág. 5)
CONTENIDO:
[fragmento relevante: to the causes and communities they care 

deeply about. This year, they volunteered over 1.2 million hours and gave $263 million (including company match) to 37,000 

nonprofit organizations in 110 countries.  

**  

In July, after we reported our earnings results—including surpassing $75 billion in annual Azure revenue for the first time—

I shared a reflectio]
4 

Another key component of earning trust is contributing to a safer online ecosystem, including protecting those who use our 

services from illegal and harmful content and conduct. We continue to take new steps to advance safety, especially for 

children, while balancing our commitments to free expression and privacy. Over the past year, we’ve focused on addressing 

risks related to abusive AI-generated content and partnered with StopNCII.org to detect victim-reported imagery in Bing.  

Our solutions, partnerships, and programs reach people and organiza tions of all abilities to help them thrive. More than 

5 million people have participated in our AI Skilling programs focused on accessibility. We launched new technology to help 

people with disabilities play, work, and live—through an Adaptive Joystick for Xbox, sign language detection in Teams, low-

vision keyboards for Surface, and AI-powered visual descriptions in Windows.  

2025 also marked the midpoint in our journey to become a carbon negative, water positive, zero waste company, and to 

protect more land than we use. We are on track to meet many of our targets and continue to accelerate progress for others.  

Our renewable energy procurement increased from 1.8 gigawatts in 2020 to 34 gigawatts in 2024, and we contracted nearly 

30 million metric tons of ca rbon removal—playing a pivotal role in scaling the carbon removal market. We provided more 

than 1.5 million people with clean water and sanitation and plan to replenish more than 100  million cubic meters of water 

around the world. We are getting closer to zero waste through new Circular Centers, which contribute to the reuse and 

recycling of nearly 91 percent of servers and components decommissioned from our datacenters. And we’ve reached nearly 

95 percent recyclability in our product packaging.  

We are learning how to make AI more sustainable by design and improve AI-powered solutions. Platforms like our Planetary 

Computer

ached nearly 

95 percent recyclability in our product packaging.  

We are learning how to make AI more sustainable by design and improve AI-powered solutions. Platforms like our Planetary 

Computer and our AI for Good Lab are helping us find new ways to address the world’s most pressing challenges.  

Making progress on these commitments takes time. But here, too—we are guided by a bold vision, thinking in decades and 

executing in quarters.  

OUR CULTURE  

Amid this rapid progress, our culture is more important than ever. The AI platform shift is reshaping not just our products 

and business models, but how we work.  

Our growth mindset is essential to our ability to continue leading this AI era. It enables us  to innovate both within Microsoft 

and with those we serve. We must be learn-it-alls, willing to experiment, guided by evaluations, and committed to continuous 

improvement. I am continually impressed by how our people do just that.  

We are focused on being  Customer Zero, applying AI to reduce toil and improve flow in our own work while creating a 

playbook we can share with the world.  

We’re also embracing a new way of working —one that expands job scopes, reduces handoffs, and gives teams tools to 

scale productivity in nonlinear ways. This isn’t just about driving efficiency. It’s about empowering our people to dream 

bigger and get to “job complete” faster, with less friction and greater impact than ever before.  

Our employees also continue to find ways to br ing their purpose and passion to the causes and communities they care 

deeply about. This year, they volunteered over 1.2 million hours and gave $263 million (including company match) to 37,000 

nonprofit organizations in 110 countries.  

**  

In July, after we reported our earnings results—including surpassing $75 billion in annual Azure revenue for the first time—

I shared a reflectio

nonprofit organizations in 110 countries.  

**  

In July, after we reported our earnings results—including surpassing $75 billion in annual Azure revenue for the first time—

I shared a reflection with all employees. Fifteen years ago, when we set out on our cloud journey, we had a bold vision, and 

we persisted through all the ups and downs.

### FUENTE [5]
DOCUMENTO: Microsoft_annual_report_2025 (pág. 14)
CONTENIDO:
[fragmento relevante: s custom -built silicon and strong partnerships 

with chip manufacturers. Azure AI Foundry is a unified platform for developers to design, customize, and manage AI 

applications and agents.  

Our server products are designed to make IT professionals, developers, and their systems more productive and efficient.]
13 

that benefit their organizations, rather than managing on-premises hardware and software. Azure revenue is mainly affected 

by infrastructure-as-a-service and platform-as-a-service consumption-based services.  

Azure AI offerings provide a competitive advantage as companies seek ways to optimize and scale their business with AI. 

We offer supercomputing power for AI at scale to run large workloads, complemented by our rapidly expanding portfolio of 

AI cloud services (including the latest models) and hardware, which includes custom -built silicon and strong partnerships 

with chip manufacturers. Azure AI Foundry is a unified platform for developers to design, customize, and manage AI 

applications and agents.  

Our server products are designed to make IT professionals, developers, and their systems more productive and efficient. 

Server software is integrated server infrastructure and middleware designed to support software applications built on the 

Windows Se rver operating system. This includes the server platform, database, business intelligence, storage, 

management and operations, virtualization, service -oriented architecture platform, security, and identity software. We also 

license standalone and software development lifecycle tools for software architects, developers, testers, and project 

managers. Server products revenue is mainly affected by purchases through volume licensing programs, licenses sold to 

OEMs, and retail packaged products. CALs provide acc ess rights to certain server products, including SQL Server and 

Windows Server, and revenue is reported along with the associated server product.  

GitHub and Nuance Healthcare include both cloud and on-premises offerings. GitHub provides a collaboration pl atform for 

developers to manage code and incorporate AI and agent -based tools across the software development lifecycle. Nuance 

Healthcare provides AI solutions to the healthcare industry.

pl atform for 

developers to manage code and incorporate AI and agent -based tools across the software development lifecycle. Nuance 

Healthcare provides AI solutions to the healthcare industry.  

Enterprise and Partner Services  

Enterprise and partner services, including Enterprise Support Services, Industry Solutions, Nuance professional services, 

Microsoft Partner Network, and Learning Experience, assist customers in developing, deploying, and managing Microsoft 

server solutions, Microsoft desktop solutions, an d Nuance conversational AI and ambient intelligent solutions, along with 

providing training and certification to developers and IT professionals on various Microsoft products.  

Competition  

Azure faces diverse competition from cloud service providers and open source offerings. Azure’s competitive advantage 

includes enabling a hybrid cloud, allowing deployment of existing datacenters with our public cloud into a single, cohesive 

infrastructure, and the ability to run at a scale that meets the needs of businesses of all sizes and complexities. Our AI 

offerings compete with AI products from hyperscalers, as well as products from other emerging competitors and other open 

source offerings, many of which are also current or potential partners. Our Azure Security offerings include our cloud security 

solution and security information and event management solution, which compete with providers in the cybersecurity and 

cloud security space. We believe our cloud’s global scale, coupled with our broad portfolio of identity and security solutions, 

allows us to effectively solve complex cybersecurity challenges for our customers and differentiates us from the competition.  

Our server products face competition from a wide variety of server operating systems and applications offered by companies 

with a range of market approaches. Vertically integrated computer manufacturers offer their own versions of the Unix 

oper

variety of server operating systems and applications offered by companies 

with a range of market approaches. Vertically integrated computer manufacturers offer their own versions of the Unix 

operating system preinstalled on server hardware and nearly all computer manufacturers offer server hardware for the Linux 

operating system.  

We compete to provide enterprise -wide computing and point solutions with numerous commercial software vendors that 

offer solutions and middleware technology platforms, software applications for connectivity, security, hosting, database, and 

e-business servers.  

Our database, business intelligence, and data warehousing solutions offerings compete with products from providers in the 

data and analytics industry. Our system management solutions compete with server management and server virtualization 

platform providers. Our products for software developers compete against offerings from major technology providers, as 

well as open source alternatives.  

We believe our server products provide customers with advantages in performance, total costs of ownership, and 

productivity by delivering superior applications, development tools, compatibility with a broad base of hardware and software 

applications, security, and manageability.

### FUENTE [6]
DOCUMENTO: Microsoft_annual_report_2025 (pág. 13)
CONTENIDO:
[fragmento relevante: (“CALs”), and other on-premises offerings.  

• Enterprise and partner services, including Enterprise Support Services, Industry Solutions, Nuance 

professional services, Microsoft Partner Network, and Learning Experience.  

Server Products and Cloud Services  

Azure is a comprehensive set of cloud services that offer developers, IT professionals, and enterprises freedom to build, 

deploy,]
12 

to offer AI -enabled insights and productivity: Talent Solutions, Marketing Solutions, Premium Subscriptions, and Sales 

Solutions. Growth will depend on our ability to increase Li nkedIn member engagement on the platform and our ability to 

continue offering insight and AI -enabled services that provide value for our members and customers. LinkedIn revenue is 

mainly affected by demand from enterprises and professionals for subscriptio ns to Talent Solutions, Sales Solutions, and 

Premium Subscriptions offerings, as well as member engagement and the quality of the sponsored content delivered to 

those members to drive Marketing Solutions.  

Dynamics Products and Cloud Services  

Dynamics pro vides cloud -based and on -premises business solutions for financial management, enterprise resource 

planning (“ERP”), customer relationship management (“CRM”), and supply chain management, as well as agentic AI and 

other low code application development pla tforms, for small and medium businesses, large organizations, and divisions of 

global enterprises. Dynamics revenue is driven by the number of users licensed and applications consumed, expansion of 

average revenue per user, and the continued shift to Dynam ics 365, a unified set of cloud -based intelligent business 

applications, including our low code development platforms, such as Power Apps and Power Automate.  

Competition  

Competitors to Office include software and global application vendors, web -based and mobile application companies, AI -

first application companies, as well as local application developers. We compete by providing secure, integrated industry -

specific, and easy-to-use productivity and collaboration tools and services that create comprehensive solutions and work 

well with technologies our customers already have both on-premises or in the cloud.  

Windows faces competition from various software products and from alternative platforms and devices. Microsoft Defender 

for Endp

ogies our customers already have both on-premises or in the cloud.  

Windows faces competition from various software products and from alternative platforms and devices. Microsoft Defender 

for Endpoint competes with endpoint security solution providers.  

Our Enterprise Mobility + Security offerings compete with products from a range of competitors including identity vendors, 

security solution vendors, and numerous other security point solution vendors.  

LinkedIn faces competition from online professional networks; recruiting, talent management, and human resource services 

companies; job boards; companies that provide learning and development products and services; online and offline outlets 

that generate revenue from advertisers and marketers; and online and offline outlets for companies with lead generation 

and customer intelligence and insights.  

Dynamics competes with cloud-based and on-premises business solution providers.  

Intelligent Cloud  

Our Intelligent Cloud segment consists of our public, private, and hybrid server products and cloud services that power 

modern business and developers. This segment primarily comprises:  

• Server pr oducts and cloud services, including Azure and other cloud services, comprising cloud and AI 

consumption-based services, GitHub cloud services, Nuance Healthcare cloud services, virtual desktop 

offerings, and other cloud services; and Server products, comp rising SQL Server, Windows Server, Visual 

Studio, System Center, related Client Access Licenses (“CALs”), and other on-premises offerings.  

• Enterprise and partner services, including Enterprise Support Services, Industry Solutions, Nuance 

professional services, Microsoft Partner Network, and Learning Experience.  

Server Products and Cloud Services  

Azure is a comprehensive set of cloud services that offer developers, IT professionals, and enterprises freedom to build, 

deploy,

, and Learning Experience.  

Server Products and Cloud Services  

Azure is a comprehensive set of cloud services that offer developers, IT professionals, and enterprises freedom to build, 

deploy, and manage applications on an y platform or device. Customers can use Azure through our global network of 

datacenters for computing, networking, storage, mobile and web application services, AI, Internet of Things, cognitive 

services, and machine learning. Azure enables customers to devote more resources to development and use of applications

### FUENTE [7]
DOCUMENTO: Microsoft_annual_report_2025 (pág. 42)
CONTENIDO:
[fragmento relevante: 41 

Service and other revenue includes sal es from cloud -based solutions that provide customers with software, services, 

platforms, and content such as Office 365, Azure, Dynamics 365, and gaming; solution support; and consulting services. 

Service and other revenue also includes sales from online advertising and LinkedIn.  

Revenue Recognition]
41 

Service and other revenue includes sal es from cloud -based solutions that provide customers with software, services, 

platforms, and content such as Office 365, Azure, Dynamics 365, and gaming; solution support; and consulting services. 

Service and other revenue also includes sales from online advertising and LinkedIn.  

Revenue Recognition  

Revenue is recognized upon transfer of control of promised products or services to customers in an amount that reflects 

the consideration we expect to receive in exchange for those products or services. We enter into contracts that can include 

various combinations of products and services, which are generally capable of being distinct and accounted for as separate 

performance obligations. Revenue is recognized net of allowances for returns and any taxes collect ed from customers, 

which are subsequently remitted to governmental authorities.  

Nature of Products and Services  

Licenses for on-premises software provide the customer with a right to use the software as it exists when made available 

to the customer. Cust omers may purchase perpetual licenses or subscribe to licenses, which provide customers with the 

same functionality and differ mainly in the duration over which the customer benefits from the software. Revenue from 

distinct on-premises licenses is recognized upfront at the point in time when the software is made available to the customer. 

In cases where we allocate revenue to software updates, primarily because the updates are provided at no additional 

charge, revenue is recognized as the updates are provided, which is generally ratably over the estimated life of the related 

device or license.  

Cloud services, which allow customers to use hosted software over the contract period without taking possession of the 

software, are provided on either a subscription  or consumption basis. Revenue related to cloud services provided on a

se hosted software over the contract period without taking possession of the 

software, are provided on either a subscription  or consumption basis. Revenue related to cloud services provided on a 

subscription basis is recognized ratably over the contract period. Revenue related to cloud services provided on a 

consumption basis, such as the amount of storage used in a period, is recognized based on the customer utilization of such 

resources. When cloud services require a significant level of integration and interdependency with software and the 

individual components are not considered distinct, all revenue is recognized over the period in which the cloud services are 

provided.  

Certain volume licensing programs, including Enterprise Agreements, include on-premises licenses combined with Software 

Assurance (“SA”). SA conveys rights to new software and upgrades released over the contract period and provides support, 

tools, and training to help customers deploy and use products more efficiently. On-premises licenses are considered distinct 

performance obligations when sold with SA. Revenue allocated to SA is generally recognized ratabl y over the contract 

period as customers simultaneously consume and receive benefits, given that SA comprises distinct performance 

obligations that are satisfied over time.  

Revenue from search advertising is recognized when the advertisement appears in the  search results or when the action 

necessary to earn the revenue has been completed. Revenue from consulting services is recognized as services are 

provided.  

Our hardware is generally highly dependent on, and interrelated with, the underlying operating sy stem and cannot function 

without the operating system. In these cases, the hardware and software license are accounted for as a single performance 

obligation and revenue is recognized at the point in time when ownership is transferred to resellers or direc tly to end 

cust

ardware and software license are accounted for as a single performance 

obligation and revenue is recognized at the point in time when ownership is transferred to resellers or direc tly to end 

customers through retail stores and online marketplaces.  

Refer to Note 18 – Segment Information and Geographic Data for further information, including revenue by significant 

product and service offering.

### FUENTE [8]
DOCUMENTO: Microsoft_annual_report_2025 (pág. 3)
CONTENIDO:
[fragmento relevante: This year alone, we added over two gigawatts of 

new capacity. Every Azure region is now AI -first and can support liquid cooling, increasing the fungibility and the fl exibility 

of our fleet. And just last month, we announced the world’s most powerful AI datacenter, Fairwater in southeastern 

Wisconsin, which will deliver 10x the performance of the world’s fastest supercomputer today.  

We a]
2 

Quality  

With QEI, we created frameworks that increase accountability and  accelerate progress against our engineering objectives 

to ensure we deliver durable, high quality-experiences at global scale. This includes improvements to change management, 

incident management, platform resiliency, and service health.  

Together, these initiatives are laying the foundation for a renaissance of our engineering culture, where we build planet -

scale systems that power the world, with the security and quality they require.  

AI innovation  

At the same time, we have made major advances in AI in novation, including across two foundational areas: our Cloud and 

AI infrastructure, and our family of Copilots and agents.  

Our Cloud and AI infrastructure  

We continue to lead the AI infrastructure wave. We opened new datacenters across six continents and  now operate more 

than 400 datacenters in 70 regions, more than any other cloud provider. This year alone, we added over two gigawatts of 

new capacity. Every Azure region is now AI -first and can support liquid cooling, increasing the fungibility and the fl exibility 

of our fleet. And just last month, we announced the world’s most powerful AI datacenter, Fairwater in southeastern 

Wisconsin, which will deliver 10x the performance of the world’s fastest supercomputer today.  

We are also driving and benefiting f rom compounding improvements in silicon, systems, and models to improve 

performance and efficiency. And we continue to invest in sovereign cloud offerings to meet the unique data residency needs 

of governments and industries worldwide.  

We have made meanin gful progress on the next frontier in cloud systems: quantum. We announced Majorana -1, the first 

quantum chip with a topological core, and deployed the world’s first operational Level  2 quantum computer in partnership 

with Atom Computing.  

In dat

uantum. We announced Majorana -1, the first 

quantum chip with a topological core, and deployed the world’s first operational Level  2 quantum computer in partnership 

with Atom Computing.  

In data and analytics, Microsoft Fabric is becoming the unified platform for the AI era. It is now our fastest-growing analytics 

product ever, with 25,000 paid customers. OneLake spans all databases and clouds, including Power BI semantic models, 

making it the best foundation for building enterprise AI applications.  

We also introduced Azure AI Foundry, a platform to design, customize, and run powerful AI apps and agents. Foundry 

includes access to more than 11,000 models from partners like OpenAI, Cohere, DeepSeek, Meta, Mistral, xAI, and others, 

ensuring our customers can choose from the best frontier and open models in one place. Already, 80 percent of the Fortune 

500 use Foundry for their AI workloads.  

And this fall, we introduced our first in-house models: MAI-1 preview, our first foundation model trained end-to-end in-house, 

as well as MAI-Voice-1 for natural voice generation and MAI-Image-1 for image generation.  

Copilots and agents  

Our Copilot family of products is helping people thrive at home, at school, and at work. This year, we surpassed 100 million 

monthly active users across both commercial and consumer.  

We rolled out a major update to Microsoft 365 Copilot this spring, bringing together chat, search, create, notebooks, and 

role-specific agents like Analyst and Researcher into a single experience. And earlier this month, we announced Agent 

Mode, which allows you to start with a simple prompt and then work iteratively with Copilot —steering it as it orchestrates 

multistep tasks to deliver high-quality Office documents, spreadsheets, and presentations. 

Give Copilot a prompt like, “Run a full analysis on this sales data set. I want to understand some important insights to help 

me mak

high-quality Office documents, spreadsheets, and presentations. 

Give Copilot a prompt like, “Run a full analysis on this sales data set. I want to understand some important insights to help 

me make decisions about my business. Make it visual.” Agent Mode gets to work deciding which formulas to use, producing 

new sheets, and creating data visualizations. It’s pretty remarkable.

### FUENTE [9]
DOCUMENTO: Microsoft_annual_report_2025 (pág. 72)
CONTENIDO:
[fragmento relevante: 22      $ 211,915   

                          

Our Microsoft Cloud revenue, which includes Microsoft 365 Commercial cloud, Azure and other cloud services, the 

commercial portion of LinkedIn, and Dynamics 365, was $168.9 billion, $137.7 billion, and $111.6 billion in fiscal years 2025,]
71 

Revenue, classified by significant product and service offerings, was as follows:  

  

(In millions)                      

   

        

Year Ended June 30,    2025      2024      2023   

        

Server products and cloud services    $ 98,435      $ 79,828      $ 65,007   

Microsoft 365 Commercial products and cloud services      87,767        76,969        66,949   

Gaming      23,455        21,503        15,466   

LinkedIn      17,812        16,372        14,989   

Windows and Devices      17,314        17,026        17,147   

Search and news advertising      13,878        12,306        12,125   

Dynamics products and cloud services      7,827        6,831        5,796   

Enterprise and partner services      7,760        7,594        7,900   

Microsoft 365 Consumer products and cloud services      7,404        6,648        6,417   

Other      72        45        119   

                   

Total    $ 281,724      $ 245,122      $ 211,915   

                          

Our Microsoft Cloud revenue, which includes Microsoft 365 Commercial cloud, Azure and other cloud services, the 

commercial portion of LinkedIn, and Dynamics 365, was $168.9 billion, $137.7 billion, and $111.6 billion in fiscal years 2025, 

2024, and 2023, respectively. These amounts are included in Microsoft 365 Commercial products and cloud services, Server 

products and cloud services, LinkedIn, and Dynamics products and cloud services in the table above.  

Assets are not allocated to segments for internal r eporting presentations. A portion of amortization and depreciation is 

included with various other costs in an overhead allocation to each segment. It is impracticable for us to separately identif y 

the amount of amortization and depreciation by segment that is included in the measure of segment profit or loss.  

Long-lived assets, excluding financial instruments and tax assets, classified by the location of the controlling statutory

n by segment that is included in the measure of segment profit or loss.  

Long-lived assets, excluding financial instruments and tax assets, classified by the location of the controlling statutory 

company and with countries over 10% of the total shown separately, were as fo llows:  

  

(In millions)                      

   

        

June 30,    2025      2024      2023   

        

United States    $ 230,069      $ 186,106      $ 114,380   

Other countries      141,833        115,263        72,859   

                   

Total    $  371,902      $  301,369      $  187,239

### FUENTE [10]
DOCUMENTO: Microsoft_annual_report_2025 (pág. 20)
CONTENIDO:
19 

The Independent Software Vendor Royalty Program enables partners to integrate Microsoft products into other applications 

and then license the unified business solution to their end users.  

GOVERNMENT REGULATION  

We are subject to a wide range of laws, regu lations, and legal requirements in the U.S. and globally, including those that 

may apply to our products and online services offerings, and those that impose requirements related to user privacy, 

telecommunications, data storage and protection, advertising , and online content. These requirements are continually 

evolving, and they can be unclear and vary significantly across jurisdictions.  We have implemented comprehensive 

compliance programs across our operations to adapt to these changes and to maintain cu stomer and regulator 

confidence. We monitor regulatory developments around the world and implement policies, controls, and technical 

safeguards so that our operations, products, and services meet applicable legal standards. Our business teams, with legal 

support, manage the compliance programs and prepare external regulatory and commercial reporting, and our internal audit 

teams conduct reviews of the programs and processes. While we have a unified approach to regulatory compliance, some 

of the programs and processes are tailored to meet specific regulatory obligations, such as with the creation of independent 

compliance functions required by the European Union (“EU”) Digital Markets Act and the EU Digital Services Act, which 

oversee, monitor, and assess the company’s compliance with these acts.  

For a description of the risks we face related to regulatory matters, refer to Risk Factors in our fiscal year 2025 Form  10K.  

AVAILABLE INFORMATION  

Our Internet address is www.microsoft.com. At our Investor Relatio ns website, www.microsoft.com/investor, we make 

availa

Risk Factors in our fiscal year 2025 Form  10K.  

AVAILABLE INFORMATION  

Our Internet address is www.microsoft.com. At our Investor Relatio ns website, www.microsoft.com/investor, we make 

available free of charge a variety of information for investors. Our goal is to maintain the Investor Relations website as a 

portal through which investors can easily find or navigate to pertinent information  about us, including:  

• Our annual report on Form 10 -K, quarterly reports on Form 10 -Q, current reports on Form 8 -K, and any 

amendments to those reports, as soon as reasonably practicable after we electronically file that material with 

or furnish it to the Securities and Exchange Commission (“SEC”) at www.sec.gov.  

• Information on our business strategies, financial results, and metrics for investors.  

• Announcements of investor conferences, speeches, and events at which our executives talk about our product, 

service, and competitive strategies. Archives of these events are also available.  

• Press releases on quarterly earnings, product and service announcements, legal developments, and 

international news.  

• Corporate governance information including our  articles of incorporation, bylaws, governance guidelines, 

committee charters, codes of conduct and ethics, global corporate social responsibility initiatives, and other 

governance-related policies.  

• Other news and announcements that we may post from tim e to time that investors might find useful or 

interesting.  

• Opportunities to sign up for email alerts to have information pushed in real time.  

We publish a variety of reports and resources related to our Corporate Social Responsibility programs and progress on our 

Reports Hub website, www.microsoft.com/corporate -responsibility/reports-hub, including reports on responsible AI, 

sustainability, responsible sourcing, accessibility, digital trust, and public policy engagement.  

The information fo

om/corporate -responsibility/reports-hub, including reports on responsible AI, 

sustainability, responsible sourcing, accessibility, digital trust, and public policy engagement.  

The information found on these websites is not part of, or incorporated by reference into, this or any other report we file 

with, or furnish to, the SEC. In addition to these channels, we use social media to communicate to the public. It is possible  

that the information we post on socia l media could be deemed to be material to investors. We encourage investors, the 

media, and others interested in Microsoft to review the information we post on the social media channels listed on our 

Investor Relations website.

### FUENTE [11]
DOCUMENTO: Microsoft_annual_report_2025 (pág. 23)
CONTENIDO:
[fragmento relevante: Microsoft Cloud revenue and revenue growth 

   

Revenue from Microsoft 365 Commercial cloud, Azure and 

other cloud services, the commercial portion of LinkedIn, 

and Dynamics 365 

    

Microsoft Cloud gross margin percentage    Gross margin percentage for our Microsoft Cloud business 

Productivity and Business Processes and Intelligent Cloud]
22 

Metrics  

We use metrics in assessing the performance of our busines s and to make informed decisions regarding the allocation of 

resources. We disclose metrics to enable investors to evaluate progress against our ambitions, provide transparency into 

performance trends, and reflect the continued evolution of our products an d services. Our commercial and other business 

metrics are fundamentally connected based on how customers use our products and services. The metrics are disclosed in 

the MD&A or the Notes to Financial Statements. Financial metrics are calculated based on fi nancial results prepared in 

accordance with accounting principles generally accepted in the United States of America (“GAAP”), and growth 

comparisons relate to the corresponding period of last fiscal year.  

In the first quarter of fiscal year 2025, we made  updates to our metrics in connection with the segment changes described 

above. These changes align our metrics with how we manage and monitor certain businesses. The key change was bringing 

the commercial components of Microsoft 365 together and creating a new Microsoft 365 Commercial cloud revenue growth 

metric. Other changes include combining Windows OEM and Devices into a single revenue growth metric that brings 

revenue from PC market-driven businesses together, as well as elevating our cloud revenue gr owth metrics to align to our 

strategic focus on cloud growth.  

Commercial  

Our commercial business primarily consists of Server products and cloud services, Microsoft 365 Commercial products and 

cloud services, the commercial portion of LinkedIn, Dynamics products and cloud services, and Enterprise and partner 

services. Our commercial metrics allow management and investors to assess the overall health of our commercial business 

and include leading indicators of future performance.  

  

Commercial remaining performance obligation 

   

Commercial portion of revenue allocated to remainin g 

perf

f our commercial business 

and include leading indicators of future performance.  

  

Commercial remaining performance obligation 

   

Commercial portion of revenue allocated to remainin g 

performance obligations, which includes unearned revenue 

and amounts that will be invoiced and recognized as 

revenue in future periods 

    

Microsoft Cloud revenue and revenue growth 

   

Revenue from Microsoft 365 Commercial cloud, Azure and 

other cloud services, the commercial portion of LinkedIn, 

and Dynamics 365 

    

Microsoft Cloud gross margin percentage    Gross margin percentage for our Microsoft Cloud business 

Productivity and Business Processes and Intelligent Cloud  

Metrics related to our Productivity and Business Processes and Intelligent Cloud segments assess the health of our core 

businesses within these segments. The metrics primarily reflect growth across  our cloud services.  

  

    

Microsoft 365 Commercial cloud revenue growth 

   

Revenue from Microsoft 365 Commercial subscriptions, 

comprising Microsoft 365 Commercial, Enterprise Mobility + 

Security, the cloud portion of Windows Commercial, the per-

user portion of Power BI, Exchange, SharePoint, Microsoft 

Teams, Microsoft 365 Security and Compliance, and 

Microsoft 365 Copilot

### FUENTE [12]
DOCUMENTO: Microsoft_annual_report_2025 (pág. 24)
CONTENIDO:
[fragmento relevante: m Dynamics 365, including a set of intelligent, 

cloud-based applications across ERP, CRM, Power Apps, 

and Power Automate 

    

Azure and other cloud services revenue growth 

   

Revenue from Azure and other cloud services, including 

cloud and AI consumption-based services, GitHub cloud 

services, Nuance Healthcare cloud services, virtual desktop 

offerings, and other cloud services]
23 

Microsoft 365 Commercial seat growth 

   

The number of Microsoft 365 Commercial seats at end of 

period where seats are paid users covered by a Microsoft 

365 Commercial subscription 

    

Microsoft 365 Consumer cloud revenue growth 

   

Revenue from Microsoft 365 Consumer subscriptions and 

other consumer services 

    

Microsoft 365 Consumer subscribers 

   

The number of Microsoft 365 Consumer subscribers at end 

of period 

    

LinkedIn revenue growth 

   

Revenue from LinkedIn, including Talent Solutions, 

Marketing Solutions, Premium Subscriptions, and Sales 

Solutions 

    

Dynamics 365 revenue growth 

   

Revenue from Dynamics 365, including a set of intelligent, 

cloud-based applications across ERP, CRM, Power Apps, 

and Power Automate 

    

Azure and other cloud services revenue growth 

   

Revenue from Azure and other cloud services, including 

cloud and AI consumption-based services, GitHub cloud 

services, Nuance Healthcare cloud services, virtual desktop 

offerings, and other cloud services 

More Personal Computing  

Metrics related to our More Personal Computing segment assess the performance of our key cons umer businesses.  

  

Windows OEM and Devices revenue growth 

   

Revenue from sales of Windows Pro and non-Pro licenses 

sold through the OEM channel and sales of first -party 

Devices, including Surface and PC accessories 

    

Xbox content and services revenue growth 

   

Revenue from Xbox content and services, comprising first- 

and third -party content (including games and in -game 

content), Xbox Game Pass and other subscriptions, Xbox 

Cloud Gaming, advertising, and other cloud services 

    

Search and news advertising revenue (ex TAC) growth 

   

Revenue from search and news advertising excluding 

traffic acquisition costs (“TAC”) paid to Bing Ads network 

publishers and news partners 

SUMMARY RESULTS OF OPERATIONS  

  

(In

growth 

   

Revenue from search and news advertising excluding 

traffic acquisition costs (“TAC”) paid to Bing Ads network 

publishers and news partners 

SUMMARY RESULTS OF OPERATIONS  

  

(In millions, except percentages and per share amounts)    2025      2024      

Percentage 

Change   

   

        

Revenue    $ 281,724      $ 245,122        15%   

Gross margin      193,893        171,008        13%   

Operating income      128,528        109,433        17%   

Net income      101,832        88,136        16%   

Diluted earnings per share      13.64        11.80        16%   

   

Fiscal Year 2025 Compared with Fiscal Year 2024  

Revenue increased $36.6  billion or 15% with growth across each of our segments. Intelligent Cloud revenue increased 

driven by Azure. Productivity and Business Processes revenue increased driven by Microsoft 365 Commercial cloud. More 

Personal Computing revenue increased driven by Gaming and Search and news advertising.  

Cost of revenue increased $13.7 billion or 19% driven by growth in Microsoft Cloud.  

Gross margin increased $22.9 billion or 13% with growth across each of our segments.

------------------------------------

PREGUNTA DEL ANALISTA: En el Annual Report 2025 de Microsoft, ¿qué cifras menciona sobre los ingresos anuales de Azure y la cantidad de modelos disponibles en su plataforma?

RESPUESTA EJECUTIVA (con citas y cifras verificadas):
```
